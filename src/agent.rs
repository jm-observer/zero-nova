use crate::message::{ContentBlock, Message, Role};
use serde_json;

use crate::provider::{LlmClient, ProviderStreamEvent};
pub use crate::tool::ToolRegistry;
use anyhow::Result;
use tokio::sync::mpsc;

pub struct AgentRuntime<C: LlmClient> {
    client: C,
    tools: ToolRegistry,
    system_prompt: String,
    config: AgentConfig,
}

pub struct AgentConfig {
    pub max_iterations: usize,
    pub model_config: crate::provider::ModelConfig,
}

impl<C: LlmClient> AgentRuntime<C> {
    pub fn new(client: C, tools: ToolRegistry, system_prompt: String, config: AgentConfig) -> Self {
        Self {
            client,
            tools,
            system_prompt,
            config,
        }
    }

    pub fn set_tools(&mut self, tools: ToolRegistry) {
        self.tools = tools;
    }

    pub fn register_tool(&mut self, tool: Box<dyn crate::tool::Tool>) {
        self.tools.register(tool);
    }

    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<crate::event::AgentEvent>,
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

        for iteration in 0..self.config.max_iterations {
            log::debug!("Agent iteration {}/{}", iteration + 1, self.config.max_iterations);
            let tool_defs = self.tools.tool_definitions();
            let mut receiver = self
                .client
                .stream(
                    &all_messages,
                    &self.system_prompt,
                    &tool_defs[..],
                    &self.config.model_config,
                )
                .await?;

            let mut current_text = String::new();
            let mut current_blocks = Vec::new();
            let mut tool_calls = Vec::new(); // (id, name, input_json)

            while let Some(event) = receiver.next_event().await? {
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
                    ProviderStreamEvent::MessageComplete { usage: _ } => {
                        // Logic handled after stream ends
                    }
                    _ => {}
                }
            }

            // End of stream for this iteration
            if !current_text.is_empty() {
                current_blocks.push(ContentBlock::Text { text: current_text });
            }

            for (id, name, input_json) in &tool_calls {
                let input_val: serde_json::Value =
                    serde_json::from_str(input_json).unwrap_or_else(|_| serde_json::json!({}));
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

            if tool_calls.is_empty() {
                // No more tools, we are done
                break;
            }

            // Execute tools (parallel)
            use futures_util::stream::{FuturesUnordered, StreamExt};
            let mut tool_results_fut = FuturesUnordered::new();

            for (id, name, input_json) in tool_calls {
                let tool_registry = &self.tools;
                let tx = event_tx.clone();
                let input_val: serde_json::Value =
                    serde_json::from_str(&input_json).unwrap_or_else(|_| serde_json::json!({}));

                tool_results_fut.push(async move {
                    let _ = tx
                        .send(crate::event::AgentEvent::ToolStart {
                            id: id.clone(),
                            name: name.clone(),
                            input: input_val.clone(),
                        })
                        .await;

                    let result = tool_registry.execute(&name, input_val).await;

                    let (content, is_error) = match result {
                        Ok(out) => (out.content, out.is_error),
                        Err(e) => (format!("Internal execution error: {}", e), true),
                    };

                    let _ = tx
                        .send(crate::event::AgentEvent::ToolEnd {
                            id: id.clone(),
                            name: name.clone(),
                            output: content.clone(),
                            is_error,
                        })
                        .await;

                    ContentBlock::ToolResult {
                        tool_use_id: id,
                        output: content,
                        is_error,
                    }
                });
            }

            let mut tool_result_blocks = Vec::new();
            while let Some(res_block) = tool_results_fut.next().await {
                tool_result_blocks.push(res_block);
            }

            let tool_res_msg = Message {
                role: Role::User, // Tool results are traditionally sent as user role or tool role
                content: tool_result_blocks,
            };
            all_messages.push(tool_res_msg.clone());
            turn_messages.push(tool_res_msg);
        }

        Ok(turn_messages)
    }
}
