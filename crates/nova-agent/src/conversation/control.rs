use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Represents the stable control state attached to a Session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    pub active_agent: String,
    #[serde(default = "default_project_dir")]
    pub project_dir: PathBuf,
    pub model_override: SessionModelOverride,
    pub last_turn_snapshot: Option<LastTurnSnapshot>,
    #[serde(default)]
    pub skill_bindings: Vec<serde_json::Value>,
    pub token_counters: SessionTokenCounters,
}

fn default_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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

impl From<&ModelRef> for nova_protocol::ModelRef {
    fn from(value: &ModelRef) -> Self {
        Self {
            provider: value.provider.clone(),
            model: value.model.clone(),
        }
    }
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
            project_dir: default_project_dir(),
            model_override: SessionModelOverride::default(),
            last_turn_snapshot: None,
            skill_bindings: Vec::new(),
            token_counters: SessionTokenCounters::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ControlState;

    #[test]
    fn deserialize_legacy_control_state_without_skill_bindings() {
        let raw = r#"{
            "active_agent":"agent-1",
            "project_dir":".",
            "model_override":{"orchestration":null,"execution":null,"updated_at":0},
            "last_turn_snapshot":null,
            "token_counters":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"updated_at":0}
        }"#;

        let state: ControlState = serde_json::from_str(raw).expect("legacy control state should be deserializable");
        assert!(state.skill_bindings.is_empty());
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

    #[test]
    fn skill_bindings_shape_is_stable_for_empty_single_and_multiple() {
        let state_empty = ControlState::new("agent-1");
        assert!(state_empty.skill_bindings.is_empty());

        let mut state_one = ControlState::new("agent-1");
        state_one.skill_bindings = vec![serde_json::json!({
            "skill_id": "skill-a",
            "name": "Skill A",
            "status": "active",
            "description": "desc"
        })];
        assert_eq!(state_one.skill_bindings.len(), 1);

        let mut state_many = ControlState::new("agent-1");
        state_many.skill_bindings = vec![
            serde_json::json!({"skill_id":"skill-a","name":"Skill A","status":"active","description":"desc-a"}),
            serde_json::json!({"skill_id":"skill-b","name":"Skill B","status":"inactive","description":"desc-b"}),
        ];
        assert_eq!(state_many.skill_bindings.len(), 2);
    }
}
