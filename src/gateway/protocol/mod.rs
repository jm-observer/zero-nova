pub mod agent;
pub mod chat;
pub mod config;
pub mod envelope;
pub mod session;
pub mod system;

pub use agent::*;
pub use chat::*;
pub use envelope::*;
pub use session::*;
pub use system::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_event() {
        let msg = GatewayMessage::new_event(MessageEnvelope::Welcome(WelcomePayload {
            require_auth: true,
            setup_required: false,
        }));
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"welcome\""));
        assert!(json.contains("\"requireAuth\":true"));
        assert!(!json.contains("\"id\":"));
    }

    #[test]
    fn test_serialize_request() {
        let msg = GatewayMessage::new(
            "req-1".to_string(),
            MessageEnvelope::Chat(ChatPayload {
                input: "hello".into(),
                session_id: None,
                agent_id: None,
                attachments: None,
            }),
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"id\":\"req-1\""));
        assert!(json.contains("\"type\":\"chat\""));
        assert!(json.contains("\"payload\":{"));
    }

    #[test]
    fn test_serialize_unit_variant() {
        let msg = GatewayMessage::new("req-2".to_string(), MessageEnvelope::SessionsList);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"sessions.list\""));
        assert!(!json.contains("\"payload\""));
    }

    #[test]
    fn test_deserialize_progress() {
        let json = r#"{
            "type": "chat.progress",
            "payload": {
                "type": "token",
                "token": "Hello",
                "sessionId": "s1"
            }
        }"#;
        let msg: GatewayMessage = serde_json::from_str(json).unwrap();
        if let MessageEnvelope::ChatProgress(p) = msg.envelope {
            assert_eq!(p.kind, "token");
            assert_eq!(p.token.unwrap(), "Hello");
            assert_eq!(p.session_id.unwrap(), "s1");
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_deserialize_agents_switch() {
        let json = r#"{
            "type": "agents.switch",
            "id": "req-3",
            "payload": {
                "sessionId": "session-1",
                "agentId": "nova"
            }
        }"#;
        let msg: GatewayMessage = serde_json::from_str(json).unwrap();
        if let MessageEnvelope::AgentsSwitch(payload) = msg.envelope {
            assert_eq!(payload.session_id, "session-1");
            assert_eq!(payload.agent_id, "nova");
        } else {
            panic!("Wrong variant");
        }
    }
}
