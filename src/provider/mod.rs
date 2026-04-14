use crate::message::Message;

use crate::provider::types::{ToolDefinition, Usage};

pub mod anthropic;
pub mod sse;
pub mod types;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>>;
}

#[async_trait]
pub trait StreamReceiver: Send {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}

#[derive(Debug, Clone)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ToolUseStart { id: String, name: String },
    ToolUseInputDelta(String),
    ToolUseEnd,
    MessageComplete { usage: Usage },
}

pub struct ModelConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
}
