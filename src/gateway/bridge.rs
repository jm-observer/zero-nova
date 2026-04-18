use crate::event::AgentEvent;
use crate::gateway::protocol::{ErrorPayload, GatewayMessage, MessageEnvelope, ProgressEvent};

/// 将 AgentEvent 转换为 GatewayMessage。
/// 消费 AgentEvent 的所有权，因为 AgentEvent 不实现 Clone，且包含 anyhow::Error。
pub fn agent_event_to_gateway(event: AgentEvent, request_id: &str, session_id: &str) -> GatewayMessage {
    let envelope = match event {
        AgentEvent::TextDelta(text) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "token".to_string(),
            session_id: Some(session_id.to_string()),
            token: Some(text),
            ..Default::default()
        }),
        AgentEvent::ToolStart { id, name, input } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_start".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name.clone()),
            tool_use_id: Some(id.clone()),
            args: Some(input),
            ..Default::default()
        }),
        AgentEvent::ToolEnd {
            id,
            name,
            output,
            is_error,
        } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_result".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name.clone()),
            tool_use_id: Some(id.clone()),
            result: Some(output.into()),
            is_error: Some(is_error),
            ..Default::default()
        }),
        AgentEvent::TurnComplete { .. } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "turn_complete".to_string(),
            session_id: Some(session_id.to_string()),
            ..Default::default()
        }),
        AgentEvent::IterationLimitReached { iterations } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "iteration_limit".to_string(),
            session_id: Some(session_id.to_string()),
            iteration: Some(iterations as i32),
            ..Default::default()
        }),
        AgentEvent::Error(e) => MessageEnvelope::Error(ErrorPayload {
            message: format!("{:#}", e),
            code: Some("AGENT_RUNTIME_ERROR".to_string()),
        }),
    };

    GatewayMessage::new(request_id.to_string(), envelope)
}
