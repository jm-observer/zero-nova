use crate::message::{ContentBlock, Message, Role};

use crate::provider::{LlmClient, ProviderStreamEvent};
use crate::tool::ToolRegistry;
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

    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<crate::event::AgentEvent>,
    ) -> Result<Vec<Message>> {
        // Build initial user message
        let user_msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: user_input.to_string(),
            }],
        };
        let mut all_messages = history.to_vec();
        all_messages.push(user_msg.clone());
        // Prepare tools definitions
        let tool_defs = self.tools.tool_definitions();
        // Call LLM stream
        let mut receiver = self
            .client
            .stream(
                &all_messages,
                &self.system_prompt,
                &tool_defs[..],
                &self.config.model_config,
            )
            .await?;
        // Simple loop: collect text deltas into a response string
        let mut response_text = String::new();
        while let Some(event) = receiver.next_event().await? {
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    response_text.push_str(&delta);
                    let _ = event_tx.send(crate::event::AgentEvent::TextDelta(delta)).await;
                }
                ProviderStreamEvent::MessageComplete { usage } => {
                    // Build final assistant message
                    let assistant_msg = Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text {
                            text: response_text.clone(),
                        }],
                    };
                    let _ = event_tx
                        .send(crate::event::AgentEvent::TurnComplete {
                            new_messages: vec![assistant_msg.clone()],
                            usage,
                        })
                        .await;
                    return Ok(vec![assistant_msg]);
                }
                _ => {}
            }
        }
        // If stream ends without MessageComplete, return accumulated message
        let assistant_msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: response_text }],
        };
        Ok(vec![assistant_msg])
    }
}
