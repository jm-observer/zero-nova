use crate::provider::types::{ToolDefinition, Usage};

pub mod anthropic;
pub mod sse;
pub mod types;
use crate::provider::types::StopReason;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
/// Trait for language model clients that can stream responses.
pub trait LlmClient: Send + Sync {
    async fn stream(
        &self,
        messages: &[crate::message::Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>>;
}

#[async_trait]
/// Trait for receiving streamed events from the LLM.
pub trait StreamReceiver: Send {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}

#[derive(Debug, Clone)]
/// Events emitted by the provider during streaming.
pub enum ProviderStreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolUseStart {
        id: String,
        name: String,
    },
    ToolUseInputDelta(String),
    ToolUseEnd,
    MessageComplete {
        usage: Usage,
        stop_reason: Option<crate::provider::types::StopReason>,
    },
}

use serde::{Deserialize, Serialize};

/// Configuration for the LLM model behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub thinking_budget: Option<u32>,
}
