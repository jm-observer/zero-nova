use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Usage statistics for a chat turn, re-exported from nova-core.
/// This mirrors `crate::provider::types::Usage` to avoid adding a dependency on nova-core.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayload {
    pub input: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
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
    /// 日志内容（仅 kind="tool_log" 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// 日志来源流: "stdout" | "stderr"（仅 kind="tool_log" 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatIntentPayload {
    pub session_id: String,
    pub intent: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletePayload {
    pub session_id: String,
    pub output: Option<String>,
    pub usage: Option<Usage>,
}

// ============================================================
// Plan 4: Skill/Tool 事件扩展 payload 类型
// ============================================================

/// Skill 激活事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillActivatedPayload {
    pub session_id: Option<String>,
    pub skill_id: String,
    pub skill_name: String,
    pub sticky: bool,
    pub reason: String,
}

/// Skill 切换事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillSwitchedPayload {
    pub session_id: Option<String>,
    pub from_skill: String,
    pub to_skill: String,
    pub reason: String,
}

/// Skill 退出事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillExitedPayload {
    pub session_id: Option<String>,
    pub skill_id: String,
    pub skill_name: String,
    pub reason: String,
}

/// Tool 解锁事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolUnlockedPayload {
    pub session_id: Option<String>,
    pub tool_name: String,
    pub source: String, // "tool_search" | "skill_activation" | "manual"
}

/// Task 状态变化事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusChangedPayload {
    pub session_id: Option<String>,
    pub task_id: String,
    pub task_subject: String,
    pub status: String,
    pub active_form: Option<String>,
    pub is_main_task: bool,
}

/// Skill 路由评估事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillRouteEvaluatedPayload {
    pub session_id: Option<String>,
    pub skill_id: String,
    pub confidence: f64,  // 0.0 - 1.0
    pub decision: String, // "activate", "keep", "fallback", etc.
    pub reasoning: String,
}

/// Skill 调用事件 payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillInvocationPayload {
    pub session_id: Option<String>,
    pub skill_id: String,
    pub skill_name: String,
    pub level: String, // "auto", "explicit", "fallback"
}

/// 工具结果 payload (结构化输出)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPayload {
    pub tool_name: String,
    pub tool_use_id: String,
    pub output: Value,
    pub is_error: bool,
    pub output_type: String, // "text", "json", "error"
}
