use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
