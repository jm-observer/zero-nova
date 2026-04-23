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
    use crate::envelope::*;
    use crate::system::*;

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
}
