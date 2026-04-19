use crate::provider::types::{ToolDefinition, Usage};

pub mod anthropic;
pub mod openai_compat;
pub mod sse;
pub mod types;

use crate::provider::types::StopReason;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Configuration for the LLM model behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Configuration for the LLM model behavior.
pub struct ModelConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    /// Anthropic: 映射为 budget_tokens; 其他 Provider: 仅作为"是否启用"的开关
    pub thinking_budget: Option<u32>,
    /// OpenAI: 映射为 reasoning_effort 参数 ("low"/"medium"/"high")
    pub reasoning_effort: Option<String>,
}
