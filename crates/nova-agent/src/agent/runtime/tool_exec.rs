use super::guards::has_loop_guard_rejection;
use super::{AgentRuntime, TurnResult};
use crate::event::AgentEvent;
use crate::loop_guard::{
    assistant_fingerprint_from_blocks, build_tool_call_signature, tool_calls_hash, LoopGuardDecision, LoopGuardState,
};
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::EnvironmentSnapshot;
use crate::provider::types::{ProviderRequestContext, StopReason, ToolDefinition, Usage};
use crate::provider::{ModelConfig, ProviderStreamEvent};
use crate::tool::read_cache::TurnReadState;
use crate::tool::ToolContext;
use anyhow::Result;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) struct ExecuteToolCallsRequest<'a> {
    pub parsed_tool_calls: Vec<(String, String, serde_json::Value)>,
    pub loop_guard_state: &'a mut LoopGuardState,
    pub turn_read_state: Arc<RwLock<TurnReadState>>,
    pub session_id: &'a str,
    pub environment: Option<EnvironmentSnapshot>,
    pub shared_environment: Option<Arc<RwLock<EnvironmentSnapshot>>>,
    pub visible_tool_names: Arc<std::collections::HashSet<String>>,
    pub event_tx: &'a mpsc::Sender<AgentEvent>,
}

pub(super) struct ExecuteTurnLoopRequest<'a> {
    pub all_messages: Vec<Message>,
    pub tool_definitions: &'a [ToolDefinition],
    pub visible_tool_names: Arc<std::collections::HashSet<String>>,
    pub iteration_budget: usize,
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub environment: Option<EnvironmentSnapshot>,
    pub event_tx: mpsc::Sender<AgentEvent>,
    pub cancellation_token: Option<CancellationToken>,
    pub model_config: &'a ModelConfig,
}

impl AgentRuntime {
    /// 执行一组工具调用并返回格式化结果。
    pub(super) async fn execute_tool_calls(&self, req: ExecuteToolCallsRequest<'_>) -> Result<Vec<ContentBlock>> {
        let ExecuteToolCallsRequest {
            parsed_tool_calls,
            loop_guard_state,
            turn_read_state,
            session_id,
            environment,
            shared_environment,
            visible_tool_names,
            event_tx,
        } = req;
        let mut tool_results_fut = FuturesUnordered::new();
        let mut indexed_results = Vec::new();

        for (call_idx, (id, name, input_val)) in parsed_tool_calls.into_iter().enumerate() {
            let signature = build_tool_call_signature(&name, &input_val);
            let signature_hash = signature.input_hash;
            let decision = loop_guard_state.evaluate_tool_call(signature.clone());
            match decision {
                LoopGuardDecision::Allow => {}
                LoopGuardDecision::AllowWithWarning { message } => {
                    let _ = event_tx
                        .send(AgentEvent::SystemLog(format!(
                            "loop_guard_triggered session_id={} reason_code=duplicate_tool_call_warning decision=warn tool={} canonical_target={:?} duplicate_count={} stalled_iteration_count={} signature_hash={}",
                            session_id,
                            name,
                            signature.canonical_primary_target,
                            loop_guard_state.duplicate_count(),
                            loop_guard_state.stalled_count(),
                            signature_hash
                        )))
                        .await;
                    let _ = event_tx.send(AgentEvent::SystemLog(message)).await;
                }
                LoopGuardDecision::Reject { message, reason_code } => {
                    let _ = event_tx
                        .send(AgentEvent::SystemLog(format!(
                            "loop_guard_triggered session_id={} reason_code={} decision=reject tool={} canonical_target={:?} duplicate_count={} stalled_iteration_count={} signature_hash={}",
                            session_id,
                            reason_code,
                            name,
                            signature.canonical_primary_target,
                            loop_guard_state.duplicate_count(),
                            loop_guard_state.stalled_count(),
                            signature_hash
                        )))
                        .await;
                    indexed_results.push((
                        call_idx,
                        ContentBlock::ToolResult {
                            tool_use_id: id,
                            output: message,
                            is_error: true,
                            images: Vec::new(),
                        },
                    ));
                    continue;
                }
            }
            let tool_registry = &self.tools;
            let tx = event_tx.clone();
            let tool_timeout_duration = self.config.tool_timeout;
            let session_id = session_id.to_string();
            let environment = environment.clone();
            let task_store = self.task_store.clone();
            let skill_registry = self.skill_registry.clone();
            let read_files = self.read_files.clone();
            let turn_read_state = turn_read_state.clone();
            let shared_environment = shared_environment.clone();
            let visible_tool_names = visible_tool_names.clone();

            tool_results_fut.push(async move {
                let _ = tx
                    .send(AgentEvent::ToolStart {
                        id: id.clone(),
                        name: name.clone(),
                        input: input_val.clone(),
                    })
                    .await;

                // 全链路追踪：每个 tool 调用包成一个 span，并把 traceparent
                // re-scope 成当前 tool 的——这样：
                //   1. 外部服务（alarm-server 等）经请求头取 traceparent 时
                //      落到本 tool 之下，体现层级；
                //   2. 嵌套子工具（如 OrchestrateTask 拉子 Agent 再调子工具）
                //      继续往下嵌套，符合"agent→tool→service"三段式。
                #[cfg(feature = "trace-propagation")]
                let tool_ctx_opt: Option<custom_utils::trace::TraceContext> = custom_utils::trace_propagation::CURRENT_TRACEPARENT
                    .try_with(|tp| tp.clone())
                    .ok()
                    .flatten()
                    .and_then(|tp| custom_utils::trace::TraceContext::from_traceparent(&tp))
                    .map(|parent| parent.child());
                #[cfg(feature = "trace-propagation")]
                let tool_start_ms = custom_utils::trace::now_ms();

                let exec_future = tool_registry.execute(
                    &name,
                    input_val.clone(),
                    Some(ToolContext {
                        event_tx: tx.clone(),
                        tool_use_id: id.clone(),
                        session_id,
                        task_store,
                        skill_registry,
                        read_files,
                        turn_read_state: Some(turn_read_state.clone()),
                        environment,
                        shared_environment,
                        cancellation_token: None,
                        visible_tool_names,
                    }),
                );

                #[cfg(feature = "trace-propagation")]
                let scoped_future = match tool_ctx_opt.as_ref() {
                    Some(ctx) => {
                        let tp = ctx.to_traceparent();
                        Box::pin(custom_utils::trace_propagation::CURRENT_TRACEPARENT
                            .scope(Some(tp), exec_future)) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
                    }
                    None => Box::pin(exec_future),
                };
                #[cfg(not(feature = "trace-propagation"))]
                let scoped_future = exec_future;

                // 两阶段 emit（用 custom_utils::trace::SpanScope 标准 API）：
                //   Phase 1：anchor with request_body=tool 入参，state=in_flight；
                //   Phase 2：finalize with response_body=tool 输出 + 真实 end_ms。
                // trace-hub INSERT OR REPLACE 自动合并到同 span_id。
                #[cfg(feature = "trace-propagation")]
                let tool_scope = tool_ctx_opt.clone().map(|ctx| {
                    let s = custom_utils::trace::SpanScope::new(ctx, "tool_call")
                        .with_summary(serde_json::json!({
                            "tool": name,
                            "tool_use_id": id,
                        }))
                        .with_request_body(
                            serde_json::to_string(&input_val).unwrap_or_default(),
                        );
                    s.emit_start();
                    s
                });
                let _ = tool_start_ms; // start_ms 现在由 SpanScope 持有，保留变量以便其它日志保持兼容

                let result = timeout(tool_timeout_duration, scoped_future).await;

                let (content, is_error, child_session, images) = match result {
                    Ok(Ok(out)) => (out.content, out.is_error, out.child_session, out.images),
                    Ok(Err(e)) => (format!("Internal execution error: {}", e), true, None, Vec::new()),
                    Err(_) => ("Tool execution timed out".to_string(), true, None, Vec::new()),
                };

                #[cfg(feature = "trace-propagation")]
                if let Some(scope) = tool_scope {
                    let status = if is_error {
                        custom_utils::trace::SpanStatus::Error("tool error".to_string())
                    } else {
                        custom_utils::trace::SpanStatus::Ok
                    };
                    scope.emit_end(
                        Some(content.clone()),
                        status,
                        Some(serde_json::json!({
                            "is_error": is_error,
                            "output_chars": content.chars().count(),
                            "images": images.len(),
                        })),
                    );
                }
                let content = if let (Some(injector), Some(skill_registry)) =
                    (self.side_channel_injector.as_ref(), self.skill_registry.as_ref())
                {
                    injector.inject_into_tool_result(&content, skill_registry.as_ref())
                } else {
                    content
                };

                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: id.clone(),
                        name: name.clone(),
                        output: content.clone(),
                        is_error,
                    })
                    .await;

                // 工具声明子会话副作用时额外发一个事件。必须排在 ToolEnd 之后，
                // 保证消费者按"工具已结束 → 触发副作用"顺序处理。
                if let Some(req) = child_session {
                    let _ = tx
                        .send(AgentEvent::ChildSessionRequest {
                            tool_use_id: id.clone(),
                            tool_name: name.clone(),
                            seed_user_message: req.seed_user_message,
                            metadata: req.metadata,
                        })
                        .await;
                }

                (
                    call_idx,
                    ContentBlock::ToolResult {
                        tool_use_id: id,
                        output: self.compact_tool_output(&name, is_error, &content),
                        is_error,
                        images,
                    },
                )
            });
        }

        while let Some(res) = tool_results_fut.next().await {
            indexed_results.push(res);
        }
        indexed_results.sort_by_key(|&(idx, _)| idx);

        Ok(indexed_results.into_iter().map(|(_, b)| b).collect())
    }

    pub(super) async fn execute_turn_loop(&self, req: ExecuteTurnLoopRequest<'_>) -> Result<TurnResult> {
        let ExecuteTurnLoopRequest {
            mut all_messages,
            tool_definitions,
            visible_tool_names,
            iteration_budget,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config,
        } = req;
        // 工具集合需在轮内可变：ToolSearch 解析的 deferred 工具必须在下一次
        // LLM 调用前进入 `tool_definitions`，并对 ToolInfo 可见，否则模型会
        // 反复尝试一个已"加载"却不可调用的工具。
        let mut tool_definitions: Vec<ToolDefinition> = tool_definitions.to_vec();
        let mut visible_tool_names = visible_tool_names;
        let mut loop_guard_state = LoopGuardState::new(self.config.loop_guard.clone());
        let turn_read_state = Arc::new(RwLock::new(TurnReadState::default()));

        let mut turn_messages = Vec::new();
        let mut cumulative_usage = Usage::default();
        let mut completed_naturally = false;
        let mut final_provider_request_body: Option<Value> = None;
        let mut final_provider_response_body: Option<Value> = None;
        let shared_environment = environment.clone().map(|env| Arc::new(RwLock::new(env)));

        for iteration in 0..iteration_budget {
            if let Some(ref token) = cancellation_token {
                if token.is_cancelled() {
                    return Ok(TurnResult {
                        messages: turn_messages,
                        usage: cumulative_usage,
                        provider_request_body: final_provider_request_body,
                        provider_response_body: final_provider_response_body,
                    });
                }
            }

            let _ = event_tx
                .send(AgentEvent::Iteration {
                    current: iteration + 1,
                    total: iteration_budget,
                })
                .await;

            // 刷新工具视图：上一轮 ToolSearch 解析的 deferred 工具在此进入
            // 本次请求的 tools 集合与可见集合，使其可被调用与查询。
            // iteration 0 沿用 prepare_turn 已计算好的初始视图。
            if iteration > 0 {
                let turn_view = self.tools.get_turn_view(session_id, true, true, true).await;
                tool_definitions = turn_view.loaded.clone();
                visible_tool_names = Arc::new(turn_view.loaded.iter().map(|d| d.name.clone()).collect());
            }

            let mut receiver = match self
                .client
                .stream(
                    &all_messages,
                    &tool_definitions,
                    model_config,
                    &ProviderRequestContext {
                        session_id: if session_id.trim().is_empty() {
                            None
                        } else {
                            Some(session_id.to_string())
                        },
                        agent_id: agent_id.to_string(),
                        message_id: Uuid::new_v4().to_string(),
                    },
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = format!("Failed to start stream: {}", e);
                    log::error!("{}", err_msg);
                    let _ = event_tx.send(AgentEvent::SystemLog(err_msg)).await;
                    return Err(e);
                }
            };

            let mut current_text = String::new();
            let mut current_thinking = String::new();
            let mut tool_calls: Vec<(String, String, String)> = Vec::new();
            let mut iter_usage = Usage::default();
            let mut last_stop_reason: Option<StopReason> = None;
            // 本次 LLM 调用开始时间：宿主拿到 ProviderHttpTrace 事件时已晚于 end，
            // 无法回推 start，故由这里记下，连同 end 一起 emit 出去（供 trace-hub 算时长）。
            let iter_started_ms = chrono::Utc::now().timestamp_millis();

            while let Some(event) = receiver
                .next_event()
                .await
                .inspect_err(|e| log::error!("Error receiving event: {}", e))?
            {
                if let Some(ref token) = cancellation_token {
                    if token.is_cancelled() {
                        return Ok(TurnResult {
                            messages: turn_messages,
                            usage: cumulative_usage,
                            provider_request_body: final_provider_request_body,
                            provider_response_body: final_provider_response_body,
                        });
                    }
                }

                match event {
                    ProviderStreamEvent::ThinkingDelta(delta) => {
                        current_thinking.push_str(&delta);
                        let _ = event_tx.send(AgentEvent::ThinkingDelta(delta)).await;
                    }
                    ProviderStreamEvent::TextDelta(delta) => {
                        current_text.push_str(&delta);
                        let _ = event_tx.send(AgentEvent::TextDelta(delta)).await;
                    }
                    ProviderStreamEvent::ToolUseStart { id, name } => {
                        tool_calls.push((id, name, String::new()));
                    }
                    ProviderStreamEvent::ToolUseInputDelta(delta) => {
                        if let Some(last) = tool_calls.last_mut() {
                            last.2.push_str(&delta);
                        }
                    }
                    ProviderStreamEvent::MessageComplete { usage, stop_reason } => {
                        iter_usage = usage;
                        last_stop_reason = stop_reason;
                    }
                    _ => {}
                }
            }
            final_provider_request_body = receiver.request_body();
            final_provider_response_body = receiver.response_body();

            // 本次 LLM 调用的完整 HTTP trace 透传给宿主（全链路追踪捕获 body）。
            // 同时（trace-propagation 启用时）由 nova 直接 emit `llm_call` span：
            // 此处的 CURRENT_TRACEPARENT 才是真正的当前父（主 agent 的 turn / 子
            // agent 的 turn / tool 内部嵌套），bridge-claw 那边的 record_llm_span
            // 只能看到外层 turn，无法正确表达 subagent 的 llm 该挂哪。
            let iter_ended_ms = chrono::Utc::now().timestamp_millis();
            if let (Some(req_body), Some(resp_body)) = (
                final_provider_request_body.as_ref(),
                final_provider_response_body.as_ref(),
            ) {
                #[cfg(feature = "trace-propagation")]
                if let Some(parent) = custom_utils::trace_propagation::CURRENT_TRACEPARENT
                    .try_with(|tp| tp.clone())
                    .ok()
                    .flatten()
                    .and_then(|tp| custom_utils::trace::TraceContext::from_traceparent(&tp))
                {
                    let ctx = parent.child();
                    let model = req_body
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    custom_utils::trace::record_llm_call(custom_utils::trace::LlmCall {
                        ctx,
                        model,
                        request_body: serde_json::to_string(req_body).unwrap_or_default(),
                        response_body: serde_json::to_string(resp_body).unwrap_or_default(),
                        start_ms: iter_started_ms,
                        end_ms: iter_ended_ms,
                        status: custom_utils::trace::SpanStatus::Ok,
                    });
                }
                let _ = event_tx
                    .send(AgentEvent::ProviderHttpTrace {
                        request_body: req_body.clone(),
                        response_body: resp_body.clone(),
                        start_ms: iter_started_ms,
                        end_ms: iter_ended_ms,
                    })
                    .await;
            }

            cumulative_usage.input_tokens += iter_usage.input_tokens;
            cumulative_usage.output_tokens += iter_usage.output_tokens;
            cumulative_usage.cache_creation_input_tokens = match (
                cumulative_usage.cache_creation_input_tokens,
                iter_usage.cache_creation_input_tokens,
            ) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            cumulative_usage.cache_read_input_tokens = match (
                cumulative_usage.cache_read_input_tokens,
                iter_usage.cache_read_input_tokens,
            ) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };

            let mut current_blocks = Vec::new();
            if !current_thinking.is_empty() {
                current_blocks.push(ContentBlock::Thinking {
                    thinking: current_thinking,
                });
            }
            if !current_text.is_empty() {
                current_blocks.push(ContentBlock::Text { text: current_text });
            }

            let parsed_tool_calls: Vec<(String, String, serde_json::Value)> = tool_calls
                .into_iter()
                .map(|(id, name, input_json)| {
                    let input_val: serde_json::Value = match serde_json::from_str(&input_json) {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("Failed to parse tool input JSON: {}. Content: {}", e, input_json);
                            serde_json::json!({ "__error": format!("Invalid JSON: {}", e) })
                        }
                    };
                    (id, name, input_val)
                })
                .collect();

            for (id, name, input_val) in &parsed_tool_calls {
                current_blocks.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input_val.clone(),
                });
            }

            let assistant_msg = Message::new(Role::Assistant, current_blocks, chrono::Utc::now().timestamp_millis());
            all_messages.push(assistant_msg.clone());
            turn_messages.push(assistant_msg);

            if last_stop_reason == Some(StopReason::MaxTokens) {
                let is_truncated = if parsed_tool_calls.is_empty() {
                    true
                } else if let Some((_, _, last_val)) = parsed_tool_calls.last() {
                    last_val.get("__error").is_some()
                } else {
                    true
                };

                if is_truncated {
                    all_messages.push(Message::new(
                        Role::User,
                        vec![ContentBlock::Text {
                            text: "Please continue your last tool call or response.".to_string(),
                        }],
                        chrono::Utc::now().timestamp_millis(),
                    ));
                    continue;
                }
            }

            if parsed_tool_calls.is_empty() {
                completed_naturally = true;
                let _ = event_tx.send(AgentEvent::TextDelta("".to_string())).await;
                break;
            }

            let assistant_fp = assistant_fingerprint_from_blocks(all_messages.last().map_or(&[], |m| &m.content));
            let signatures = parsed_tool_calls
                .iter()
                .map(|(_, name, input)| build_tool_call_signature(name, input))
                .collect::<Vec<_>>();
            let calls_hash = tool_calls_hash(&signatures);
            let current_environment = if let Some(env) = &shared_environment {
                Some(env.read().await.clone())
            } else {
                environment.clone()
            };
            let tool_result_blocks = self
                .execute_tool_calls(ExecuteToolCallsRequest {
                    parsed_tool_calls,
                    loop_guard_state: &mut loop_guard_state,
                    turn_read_state: turn_read_state.clone(),
                    session_id,
                    environment: current_environment,
                    shared_environment: shared_environment.clone(),
                    visible_tool_names: visible_tool_names.clone(),
                    event_tx: &event_tx,
                })
                .await?;

            let tool_res_msg = Message::new(Role::User, tool_result_blocks, chrono::Utc::now().timestamp_millis());
            let has_guard_rejection = has_loop_guard_rejection(&tool_res_msg.content);
            all_messages.push(tool_res_msg.clone());
            turn_messages.push(tool_res_msg);

            if !has_guard_rejection && loop_guard_state.detect_stalled_iteration(assistant_fp, calls_hash) {
                let _ = event_tx
                    .send(AgentEvent::SystemLog(format!(
                        "loop_guard_triggered session_id={} reason_code=stalled_iteration_abort decision=reject tool=<none> canonical_target=<none> duplicate_count={} stalled_iteration_count={} signature_hash={}",
                        session_id,
                        loop_guard_state.duplicate_count(),
                        loop_guard_state.stalled_count(),
                        calls_hash
                    )))
                    .await;
                completed_naturally = true;
                break;
            }
        }

        if !completed_naturally {
            let _ = event_tx
                .send(AgentEvent::IterationLimitReached {
                    iterations: iteration_budget,
                })
                .await;
            let _ = event_tx
                .send(AgentEvent::TurnComplete {
                    new_messages: turn_messages.clone(),
                    usage: cumulative_usage.clone(),
                })
                .await;
        }

        Ok(TurnResult {
            messages: turn_messages,
            usage: cumulative_usage,
            provider_request_body: final_provider_request_body,
            provider_response_body: final_provider_response_body,
        })
    }
}
