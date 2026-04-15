use crate::message::{ContentBlock, Message, Role};
use serde_json;

use crate::provider::{LlmClient, ProviderStreamEvent};
pub use crate::tool::ToolRegistry;
use anyhow::Result;
use tokio::sync::mpsc;

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

            log::info!("Stream started, receiving events...");
            let mut current_text = String::new();
            let mut current_blocks = Vec::new();
            let mut tool_calls = Vec::new(); // (id, name, input_json)
            let mut last_usage = None;

            while let Some(event) = receiver
                .next_event()
                .await
                .inspect_err(|e| log::error!("Error receiving event: {}", e))?
            {
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
                    ProviderStreamEvent::MessageComplete { usage } => {
                        last_usage = Some(usage);
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
                // No more tools, send TurnComplete and finish
                if let Some(usage) = last_usage {
                    let _ = event_tx
                        .send(crate::event::AgentEvent::TurnComplete {
                            new_messages: turn_messages.clone(),
                            usage,
                        })
                        .await;
                }
                break;
            }

            // Execute tool calls (parallel) - encapsulated for clarity
            use futures_util::stream::{FuturesUnordered, StreamExt};
            let mut tool_results_fut = FuturesUnordered::new();

            for (id, name, input_json) in tool_calls {
                let tool_registry = &self.tools;
                let tx = event_tx.clone();
                let input_val: serde_json::Value =
                    serde_json::from_str(&input_json).unwrap_or_else(|_| serde_json::json!({}));

                tool_results_fut.push(async move {
                    log::info!("Executing tool: {} with input: {}", name, input_json);
                    let _ = tx
                        .send(crate::event::AgentEvent::ToolStart {
                            id: id.clone(),
                            name: name.clone(),
                            input: input_val.clone(),
                        })
                        .await;

                    let result = tool_registry.execute(&name, input_val).await;
                    match &result {
                        Ok(_out) => log::info!("Tool {} executed successfully", name),
                        Err(e) => log::error!("Tool {} execution failed: {}", name, e),
                    }

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

            // Send TurnComplete with usage from last stream if this is the final iteration
            if iteration == self.config.max_iterations - 1 {
                if let Some(usage) = last_usage {
                    let _ = event_tx
                        .send(crate::event::AgentEvent::TurnComplete {
                            new_messages: turn_messages.clone(),
                            usage,
                        })
                        .await;
                }
            }
        }

        Ok(turn_messages)
    }
}
