use super::guards::has_loop_guard_rejection;
use super::{AgentRuntime, TurnResult};
use crate::event::AgentEvent;
use crate::loop_guard::{
    assistant_fingerprint_from_blocks, build_tool_call_signature, tool_calls_hash, LoopGuardDecision, LoopGuardState,
};
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::EnvironmentSnapshot;
use crate::provider::types::{ProviderRequestContext, StopReason, ToolDefinition, Usage};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent};
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

impl<C: LlmClient> AgentRuntime<C> {
    /// 执行一组工具调用并返回格式化结果。
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_tool_calls(
        &self,
        parsed_tool_calls: Vec<(String, String, serde_json::Value)>,
        loop_guard_state: &mut LoopGuardState,
        turn_read_state: Arc<RwLock<TurnReadState>>,
        session_id: &str,
        environment: Option<EnvironmentSnapshot>,
        shared_environment: Option<Arc<RwLock<EnvironmentSnapshot>>>,
        visible_tool_names: Arc<std::collections::HashSet<String>>,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> Result<Vec<ContentBlock>> {
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

                let result = timeout(
                    tool_timeout_duration,
                    tool_registry.execute(
                        &name,
                        input_val,
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
                    ),
                )
                .await;

                let (content, is_error) = match result {
                    Ok(Ok(out)) => (out.content, out.is_error),
                    Ok(Err(e)) => (format!("Internal execution error: {}", e), true),
                    Err(_) => ("Tool execution timed out".to_string(), true),
                };
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

                (
                    call_idx,
                    ContentBlock::ToolResult {
                        tool_use_id: id,
                        output: self.compact_tool_output(&name, is_error, &content),
                        is_error,
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_turn_loop(
        &self,
        mut all_messages: Vec<Message>,
        tool_definitions: &[ToolDefinition],
        visible_tool_names: Arc<std::collections::HashSet<String>>,
        iteration_budget: usize,
        session_id: &str,
        agent_id: Option<&str>,
        environment: Option<EnvironmentSnapshot>,
        event_tx: mpsc::Sender<AgentEvent>,
        cancellation_token: Option<CancellationToken>,
        model_config: &ModelConfig,
    ) -> Result<TurnResult> {
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

            let mut receiver = match self
                .client
                .stream(
                    &all_messages,
                    tool_definitions,
                    model_config,
                    &ProviderRequestContext {
                        session_id: if session_id.trim().is_empty() {
                            None
                        } else {
                            Some(session_id.to_string())
                        },
                        agent_id: agent_id.map(|id| id.to_string()),
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
                .execute_tool_calls(
                    parsed_tool_calls,
                    &mut loop_guard_state,
                    turn_read_state.clone(),
                    session_id,
                    current_environment,
                    shared_environment.clone(),
                    visible_tool_names.clone(),
                    &event_tx,
                )
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
