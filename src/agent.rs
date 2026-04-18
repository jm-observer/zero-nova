use crate::message::{ContentBlock, Message, Role};
use serde_json;

use crate::provider::{LlmClient, ProviderStreamEvent};
pub use crate::tool::ToolRegistry;
use anyhow::Result;
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// Runtime for the zero-nova agent.
pub struct AgentRuntime<C: LlmClient> {
    client: C,
    tools: ToolRegistry,
    system_prompt: String,
    config: AgentConfig,
}

/// Configuration for the zero-nova agent.
pub struct AgentConfig {
    pub max_iterations: usize,
    pub model_config: crate::provider::ModelConfig,
    pub tool_timeout: Duration,
}

impl<C: LlmClient> AgentRuntime<C> {
    /// Creates a new `AgentRuntime` instance.
    pub fn new(client: C, tools: ToolRegistry, system_prompt: String, config: AgentConfig) -> Self {
        Self {
            client,
            tools,
            system_prompt,
            config,
        }
    }

    /// Sets the tool registry for this runtime.
    pub fn set_tools(&mut self, tools: ToolRegistry) {
        self.tools = tools;
    }

    /// Registers a new tool with the registry.
    pub fn register_tool(&mut self, tool: Box<dyn crate::tool::Tool>) {
        self.tools.register(tool);
    }

    /// Returns a reference to the system prompt string.
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// Returns a reference to the tool registry.
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Executes a single turn of the agent, handling LLM streaming and tool execution.
    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<crate::event::AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<Vec<Message>> {
        let mut all_messages = history.to_vec();
        // Append initial user message
        all_messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: user_input.to_string(),
            }],
        });

        let mut turn_messages = Vec::new();
        let mut cumulative_usage = crate::provider::types::Usage::default();
        let mut completed_naturally = false;

        for iteration in 0..self.config.max_iterations {
            if let Some(token) = &cancellation_token {
                if token.is_cancelled() {
                    return Ok(turn_messages);
                }
            }

            log::debug!("Agent iteration {}/{}", iteration + 1, self.config.max_iterations);
            let tool_defs = self.tools.tool_definitions();
            log::info!("Starting stream for iteration {}", iteration + 1);

            let mut receiver = self
                .client
                .stream(
                    &all_messages,
                    &self.system_prompt,
                    &tool_defs[..],
                    &self.config.model_config,
                )
                .await
                .inspect_err(|e| log::error!("Failed to start stream: {}", e))?;

            let mut current_text = String::new();
            let mut tool_calls = Vec::new(); // (id, name, input_json)
            let mut iter_usage = crate::provider::types::Usage::default();
            let mut last_stop_reason: Option<crate::provider::types::StopReason> = None;

            while let Some(event) = receiver
                .next_event()
                .await
                .inspect_err(|e| log::error!("Error receiving event: {}", e))?
            {
                if let Some(token) = &cancellation_token {
                    if token.is_cancelled() {
                        return Ok(turn_messages);
                    }
                }

                match event {
                    ProviderStreamEvent::TextDelta(delta) => {
                        current_text.push_str(&delta);
                        let _ = event_tx.send(crate::event::AgentEvent::TextDelta(delta)).await;
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

            // Accumulate usage
            cumulative_usage.input_tokens += iter_usage.input_tokens;
            cumulative_usage.output_tokens += iter_usage.output_tokens;
            cumulative_usage.cache_creation_input_tokens += iter_usage.cache_creation_input_tokens;
            cumulative_usage.cache_read_input_tokens += iter_usage.cache_read_input_tokens;

            let mut current_blocks = Vec::new();
            if !current_text.is_empty() {
                current_blocks.push(ContentBlock::Text { text: current_text });
            }

            for (id, name, input_json) in &tool_calls {
                let input_val: serde_json::Value = match serde_json::from_str(input_json) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("Failed to parse tool input JSON: {}. Content: {}", e, input_json);
                        serde_json::json!({ "__error": format!("Invalid JSON: {}", e) })
                    }
                };
                current_blocks.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input_val,
                });
            }

            let assistant_msg = Message {
                role: Role::Assistant,
                content: current_blocks,
            };
            all_messages.push(assistant_msg.clone());
            turn_messages.push(assistant_msg);

            // 3.4 MaxTokens 自动续写
            if last_stop_reason == Some(crate::provider::types::StopReason::MaxTokens) {
                let is_truncated = if tool_calls.is_empty() {
                    true
                } else {
                    // 检查最后一个 tool call 是否结束
                    let (_, _, last_json) = tool_calls.last().unwrap();
                    let trimmed = last_json.trim();
                    trimmed.is_empty() || !trimmed.ends_with('}')
                };

                if is_truncated {
                    all_messages.push(Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: "Please continue your last tool call or response.".to_string(),
                        }],
                    });
                    continue;
                }
            }

            if tool_calls.is_empty() {
                completed_naturally = true;
                let _ = event_tx
                    .send(crate::event::AgentEvent::TextDelta("".to_string())) // No-op to maintain stream if needed, but we removed TurnComplete
                    .await;
                break;
            }

            // 3.6 Tool 执行超时 & 3.1 Tool 结果顺序保持
            let mut tool_results_fut = FuturesUnordered::new();

            for (call_idx, (id, name, input_json)) in tool_calls.into_iter().enumerate() {
                let tool_registry = &self.tools;
                let tx = event_tx.clone();
                let input_val: serde_json::Value = match serde_json::from_str(&input_json) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("Failed to parse tool input JSON: {}. Content: {}", e, input_json);
                        serde_json::json!({ "__error": format!("Invalid JSON: {}", e) })
                    }
                };
                let tool_timeout_duration = self.config.tool_timeout;

                tool_results_fut.push(async move {
                    let _ = tx
                        .send(crate::event::AgentEvent::ToolStart {
                            id: id.clone(),
                            name: name.clone(),
                            input: input_val.clone(),
                        })
                        .await;

                    let result = timeout(tool_timeout_duration, tool_registry.execute(&name, input_val)).await;

                    let (content, is_error) = match result {
                        Ok(Ok(out)) => (out.content, out.is_error),
                        Ok(Err(e)) => (format!("Internal execution error: {}", e), true),
                        Err(_) => ("Tool execution timed out".to_string(), true),
                    };

                    let _ = tx
                        .send(crate::event::AgentEvent::ToolEnd {
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
                            output: content,
                            is_error,
                        },
                    )
                });
            }

            let mut indexed_results = Vec::new();
            while let Some(res) = tool_results_fut.next().await {
                if let Some(token) = &cancellation_token {
                    if token.is_cancelled() {
                        return Ok(turn_messages);
                    }
                }
                indexed_results.push(res);
            }
            indexed_results.sort_by_key(|&(idx, _)| idx);

            let tool_result_blocks: Vec<ContentBlock> = indexed_results.into_iter().map(|(_, b)| b).collect();

            let tool_res_msg = Message {
                role: Role::User,
                content: tool_result_blocks,
            };
            all_messages.push(tool_res_msg.clone());
            turn_messages.push(tool_res_msg);
        }

        if !completed_naturally {
            let _ = event_tx
                .send(crate::event::AgentEvent::IterationLimitReached {
                    iterations: self.config.max_iterations,
                })
                .await;
            let _ = event_tx
                .send(crate::event::AgentEvent::TurnComplete {
                    new_messages: turn_messages.clone(),
                    usage: cumulative_usage.clone(),
                })
                .await;
        }

        Ok(turn_messages)
    }
}
