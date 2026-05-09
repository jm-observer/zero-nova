use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 标题来源标识
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    #[default]
    Default,
    Ai,
    Manual,
}

/// 标题生成状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitleStatus {
    #[default]
    Idle,
    Pending,
    Succeeded,
    Failed,
}

/// 会话标题生成状态。
/// 用于解释 `Session.name` 是默认值还是 AI 生成结果，
/// 并记录生成过程中的重试次数、错误信息等。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TitleState {
    /// 标题来源：default / ai / manual
    #[serde(default)]
    pub source: TitleSource,
    /// 当前生成状态
    #[serde(default)]
    pub status: TitleStatus,
    /// 已尝试次数
    #[serde(default)]
    pub attempt_count: u8,
    /// 最后一次尝试时间（毫秒时间戳）
    #[serde(default)]
    pub last_attempt_at: i64,
    /// 最后一次成功时间（毫秒时间戳）
    #[serde(default)]
    pub last_success_at: Option<i64>,
    /// 最后一次失败的错误信息
    #[serde(default)]
    pub last_error: Option<String>,
    /// 基于多少条用户消息触发（用于日志和调试）
    #[serde(default)]
    pub based_on_user_message_count: usize,
}

impl TitleState {
    pub fn new_default() -> Self {
        Self::default()
    }

    pub fn set_pending(&mut self, user_message_count: usize) {
        self.status = TitleStatus::Pending;
        self.attempt_count += 1;
        self.last_attempt_at = chrono::Utc::now().timestamp_millis();
        self.based_on_user_message_count = user_message_count;
    }

    pub fn set_succeeded(&mut self) {
        self.status = TitleStatus::Succeeded;
        self.source = TitleSource::Ai;
        self.last_success_at = Some(chrono::Utc::now().timestamp_millis());
        self.last_error = None;
    }

    pub fn set_failed(&mut self, error: String) {
        self.status = TitleStatus::Failed;
        self.last_error = Some(error);
    }

    pub fn should_retry(&self) -> bool {
        self.status == TitleStatus::Failed && self.attempt_count < crate::conversation::service::TITLE_MAX_ATTEMPTS
    }
}

/// Represents the stable control state attached to a Session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    pub active_agent: String,
    #[serde(default)]
    pub project_dir: Option<PathBuf>,
    #[serde(default)]
    pub model_override: SessionModelOverride,
    #[serde(default)]
    pub last_turn_snapshot: Option<LastTurnSnapshot>,
    #[serde(default)]
    pub skill_bindings: Vec<serde_json::Value>,
    #[serde(default)]
    pub system_prompt_base_override: Option<String>,
    #[serde(default)]
    pub system_prompt_state: SystemPromptState,
    #[serde(default)]
    pub token_counters: SessionTokenCounters,
    /// 会话标题生成状态（不持久化到 SQLite，仅内存状态）。
    /// 解释 `Session.name` 是默认值还是 AI 结果。
    #[serde(default)]
    pub title_state: TitleState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionModelOverride {
    pub orchestration: Option<ModelRef>,
    pub execution: Option<ModelRef>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LastTurnSnapshot {
    pub turn_id: String,
    pub prepared_at: i64,
    pub prompt_preview: Option<serde_json::Value>,
    #[serde(default)]
    pub tools: Vec<serde_json::Value>,
    #[serde(default)]
    pub skills: Vec<serde_json::Value>,
    #[serde(default)]
    pub memory_hits: Option<Vec<serde_json::Value>>,
    pub usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemPromptState {
    /// Session 当前生效的系统提示词版本（建议由编译后的 prompt 内容哈希得到）。
    pub version: String,
    /// 最后一次替换为当前版本的时间。
    pub updated_at: i64,
    /// 最近一次用于生成当前版本的配置源修订标识（如配置哈希或 mtime 摘要）。
    pub source_revision: String,
}

/// Session 级 token 累计缓存。
/// 这是一个派生值，真实基线来自各 run 的 usage 明细。
/// 正常路径下由 turn 完成时增量更新；异常场景下可从 run usage 重建。
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
        Self::new_with_project_dir(default_agent, None)
    }

    pub fn new_with_project_dir(default_agent: &str, project_dir: Option<PathBuf>) -> Self {
        Self {
            active_agent: default_agent.to_string(),
            project_dir,
            model_override: SessionModelOverride::default(),
            last_turn_snapshot: None,
            skill_bindings: Vec::new(),
            system_prompt_base_override: None,
            system_prompt_state: SystemPromptState::default(),
            token_counters: SessionTokenCounters::default(),
            title_state: TitleState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ControlState;
    use std::path::PathBuf;

    #[test]
    fn deserialize_legacy_control_state_with_project_dir_string() {
        let raw = r#"{
            "active_agent":"agent-1",
            "project_dir":".",
            "model_override":{"orchestration":null,"execution":null,"updated_at":0},
            "last_turn_snapshot":null,
            "token_counters":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"updated_at":0}
        }"#;

        let state: ControlState = serde_json::from_str(raw).expect("legacy control state should be deserializable");
        assert_eq!(state.project_dir, Some(PathBuf::from(".")));
        assert!(state.skill_bindings.is_empty());
    }

    #[test]
    fn deserialize_legacy_control_state_without_project_dir() {
        let raw = r#"{
            "active_agent":"agent-1",
            "model_override":{"orchestration":null,"execution":null,"updated_at":0},
            "last_turn_snapshot":null,
            "token_counters":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"updated_at":0}
        }"#;

        let state: ControlState = serde_json::from_str(raw).expect("legacy control state should be deserializable");
        assert_eq!(state.project_dir, None);
        assert!(state.skill_bindings.is_empty());
    }

    #[test]
    fn new_control_state_starts_without_project_dir() {
        let state = ControlState::new("agent-1");
        assert_eq!(state.project_dir, None);
    }

    #[test]
    fn serialize_and_deserialize_control_state_with_skill_bindings() {
        let mut state = ControlState::new("agent-1");
        state.skill_bindings = vec![serde_json::json!({
            "skill_id": "skill-a",
            "name": "Skill A",
            "status": "active",
            "description": "desc"
        })];

        let encoded = serde_json::to_string(&state).expect("control state should be serializable");
        let decoded: ControlState = serde_json::from_str(&encoded).expect("control state should be deserializable");
        assert_eq!(decoded.skill_bindings.len(), 1);
        assert_eq!(decoded.skill_bindings[0]["skill_id"], "skill-a");
    }
}
