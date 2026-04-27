use serde::{Deserialize, Serialize};

/// Represents the stable control state attached to a Session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    pub active_agent: String,
    pub model_override: SessionModelOverride,
    pub last_turn_snapshot: Option<LastTurnSnapshot>,
    pub token_counters: SessionTokenCounters,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionModelOverride {
    pub orchestration: Option<ModelRef>,
    pub execution: Option<ModelRef>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastTurnSnapshot {
    pub turn_id: String,
    pub prepared_at: i64,
    pub prompt_preview: Option<serde_json::Value>, // Using Value to avoid deep dependency on protocol/core types here
    pub tools: Vec<serde_json::Value>,
    pub skills: Vec<serde_json::Value>,
    pub memory_hits: Option<Vec<serde_json::Value>>,
    pub usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionTokenCounters {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub updated_at: i64,
}

impl ControlState {
    pub fn new(default_agent: &str) -> Self {
        Self {
            active_agent: default_agent.to_string(),
            model_override: SessionModelOverride::default(),
            last_turn_snapshot: None,
            token_counters: SessionTokenCounters::default(),
        }
    }
}
