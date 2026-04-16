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

// --- Inbound payloads ---

#[derive(Debug, Deserialize)]
pub struct AuthPayload {
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatPayload {
    /// 前端字段名为 sessionId (camelCase)
    #[serde(alias = "sessionId")]
    pub session_id: Option<String>,
    /// 前端字段名为 input
    #[serde(alias = "input")]
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, alias = "agentId")]
    pub agent_id: Option<String>,
    #[serde(default, alias = "chatroomId")]
    pub chatroom_id: Option<u64>,
    #[serde(default)]
    pub attachments: Option<serde_json::Value>,
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
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub message: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub message_count: usize,
    pub created_at: i64, // unix timestamp in milliseconds
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}
