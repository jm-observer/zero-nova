use crate::event::AgentEvent;
use crate::gateway::protocol::GatewayMessage;
use serde_json::json;

/// 将 AgentEvent 转换为 GatewayMessage。
/// 消费 AgentEvent 的所有权，因为 AgentEvent 不实现 Clone，且包含 anyhow::Error。
pub fn agent_event_to_gateway(event: AgentEvent, request_id: &str, session_id: &str) -> GatewayMessage {
    match event {
        AgentEvent::TextDelta(text) => GatewayMessage {
            msg_type: "chat.progress".to_string(),
            id: request_id.to_string(),
            payload: json!({
                "session_id": session_id,
                "kind": "token",
                "text": text
            }),
        },
        AgentEvent::ToolStart { id, name, input } => GatewayMessage {
            msg_type: "chat.progress".to_string(),
            id: request_id.to_string(),
            payload: json!({
                "session_id": session_id,
                "kind": "tool_start",
                "id": id,
                "name": name,
                "input": input
            }),
        },
        AgentEvent::ToolEnd {
            id,
            name,
            output,
            is_error,
        } => GatewayMessage {
            msg_type: "chat.progress".to_string(),
            id: request_id.to_string(),
            payload: json!({
                "session_id": session_id,
                "kind": "tool_end",
                "id": id,
                "name": name,
                "output": output,
                "is_error": is_error
            }),
        },
        AgentEvent::TurnComplete { new_messages, usage } => GatewayMessage {
            msg_type: "chat.complete".to_string(),
            id: request_id.to_string(),
            payload: json!({
                "session_id": session_id,
                "messages": new_messages,
                "usage": usage
            }),
        },
        AgentEvent::Error(e) => GatewayMessage {
            msg_type: "chat.error".to_string(),
            id: request_id.to_string(),
            payload: json!({
                "session_id": session_id,
                "error": format!("{:#}", e)
            }),
        },
    }
}
