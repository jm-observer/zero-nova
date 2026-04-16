use crate::message::Message;
use crate::provider::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 统一消息信封 (与 OpenFlux 前端完全一致)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub id: String,
    pub payload: Value,
}

/// Gateway 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub host: String,          // default: "127.0.0.1"
    pub port: u16,             // default: 9090
    pub model: String,         // default: "gpt-oss-120b"
    pub max_tokens: u32,       // default: 8192
    pub max_iterations: usize, // default: 10
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9090,
            model: "gpt-oss-120b".to_string(),
            max_tokens: 8192,
            max_iterations: 10,
            api_key: None,
            base_url: None,
        }
    }
}

// --- Inbound payloads ---

#[derive(Debug, Deserialize)]
pub struct AuthPayload {
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatPayload {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionGetPayload {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionCreatePayload {
    pub name: Option<String>,
}

// --- Outbound payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct ChatProgressPayload {
    pub session_id: String,
    #[serde(flatten)]
    pub progress: ProgressType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressType {
    Token {
        text: String,
    },
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    Thinking {
        text: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletePayload {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatErrorPayload {
    pub session_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub message_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}
