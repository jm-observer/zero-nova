pub mod agent;
pub mod chat;
pub mod config;
pub mod envelope;
pub mod observability;
pub mod schema;
pub mod session;
pub mod system;
pub mod voice;

pub use agent::*;
pub use chat::*;
pub use envelope::*;
pub use observability::*;
pub use session::*;
pub use system::*;
pub use voice::*;

#[cfg(test)]
mod tests {
    use crate::chat::*;
    use crate::envelope::*;
    use crate::system::*;
    use crate::voice::*;
    use serde_json::{from_str, to_string};

    #[test]
    fn test_serialize_event() {
        let msg = GatewayMessage::new_event(MessageEnvelope::Welcome(WelcomePayload {
            require_auth: true,
            setup_required: false,
        }));
        let json = to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"welcome\""));
        assert!(json.contains("\"requireAuth\":true"));
        assert!(!json.contains("\"id\":"));
    }

    #[test]
    fn test_skill_activated_envelope() {
        let payload = SkillActivatedPayload {
            skill_id: "code-review".to_string(),
            skill_name: "Code Review".to_string(),
            sticky: true,
            reason: "auto".to_string(),
            ..Default::default()
        };
        let msg = GatewayMessage::new_event(MessageEnvelope::SkillActivated(payload));
        let json = to_string(&msg).unwrap();
        assert!(json.contains("\"skillId\":\"code-review\""));
        assert!(json.contains("\"sticky\":true"));
    }

    #[test]
    fn test_tool_unlocked_envelope() {
        let payload = ToolUnlockedPayload {
            tool_name: "TaskCreate".to_string(),
            source: "tool_search".to_string(),
            ..Default::default()
        };
        let msg = GatewayMessage::new_event(MessageEnvelope::ToolUnlocked(payload));
        let json = to_string(&msg).unwrap();
        assert!(json.contains("\"toolName\":\"TaskCreate\""));
        assert!(json.contains("\"source\":\"tool_search\""));
    }

    #[test]
    fn test_task_status_changed_envelope() {
        let payload = TaskStatusChangedPayload {
            task_id: "1".to_string(),
            task_subject: "Build project".to_string(),
            status: "completed".to_string(),
            is_main_task: true,
            ..Default::default()
        };
        let msg = GatewayMessage::new_event(MessageEnvelope::TaskStatusChanged(payload));
        let json = to_string(&msg).unwrap();
        assert!(json.contains("\"taskId\":\"1\""));
        assert!(json.contains("\"status\":\"completed\""));
        assert!(json.contains("\"isMainTask\":true"));
    }

    #[test]
    fn test_payload_integrity_no_structure_damage() {
        // Verify that all new payload types can be serialized and deserialized
        // without structural damage (Plan 4 regression test)
        let skill_payload = SkillActivatedPayload {
            skill_id: "test".to_string(),
            skill_name: "Test".to_string(),
            sticky: true,
            reason: "auto".to_string(),
            ..Default::default()
        };
        let tool_payload = ToolUnlockedPayload {
            tool_name: "Bash".to_string(),
            source: "skill_activation".to_string(),
            ..Default::default()
        };
        let task_payload = TaskStatusChangedPayload {
            task_id: "1".to_string(),
            task_subject: "Test".to_string(),
            status: "pending".to_string(),
            is_main_task: true,
            ..Default::default()
        };

        let json = to_string(&skill_payload).unwrap();
        let _restored: SkillActivatedPayload = from_str(&json).unwrap();

        let json = to_string(&tool_payload).unwrap();
        let _restored: ToolUnlockedPayload = from_str(&json).unwrap();

        let json = to_string(&task_payload).unwrap();
        let _restored: TaskStatusChangedPayload = from_str(&json).unwrap();
    }

    #[test]
    fn test_voice_error_envelope() {
        let payload = VoiceErrorPayload {
            code: VoiceErrorCode::VoiceSttTimeout,
            message: "stt request timed out".to_string(),
            capability: VoiceCapability::Stt,
            request_id: Some("req-1".to_string()),
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
        };
        let msg = GatewayMessage::new_event(MessageEnvelope::VoiceError(payload));
        let json = to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"voice.error\""));
        assert!(json.contains("\"code\":\"voice_stt_timeout\""));
        assert!(json.contains("\"capability\":\"stt\""));
    }
}
