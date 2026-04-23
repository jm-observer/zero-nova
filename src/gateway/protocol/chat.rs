use crate::provider::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayload {
    pub input: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub attachments: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String, // 'thinking' | 'tool_start' | 'tool_result' | 'token' | 'complete' | 'tool_log'
    pub session_id: Option<String>,
    pub iteration: Option<i32>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub is_error: Option<bool>,
    pub thinking: Option<String>,
    pub token: Option<String>,
    pub output: Option<String>,
    /// 日志内容（仅 kind=”tool_log” 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// 日志来源流: “stdout” | “stderr”（仅 kind=”tool_log” 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatIntentPayload {
    pub session_id: String,
    pub intent: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletePayload {
    pub session_id: String,
    pub output: Option<String>,
    pub usage: Option<Usage>,
}
