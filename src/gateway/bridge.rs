use crate::event::AgentEvent;
use crate::gateway::protocol::{
    Agent, AgentsSwitchResponse, ErrorPayload, GatewayMessage, MessageEnvelope, ProgressEvent,
};

/// 将 AgentEvent 转换为 GatewayMessage。
pub fn agent_event_to_gateway(event: AgentEvent, request_id: &str, session_id: &str) -> GatewayMessage {
    let envelope = match event {
        AgentEvent::TextDelta(text) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "token".to_string(),
            session_id: Some(session_id.to_string()),
            token: Some(text),
            ..Default::default()
        }),
        AgentEvent::ThinkingDelta(text) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "thinking".to_string(),
            session_id: Some(session_id.to_string()),
            thinking: Some(text),
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
        AgentEvent::LogDelta { id, name, log, stream } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_log".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name),
            tool_use_id: Some(id),
            log: Some(log),
            stream: Some(stream),
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
        AgentEvent::SystemLog(log) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "system_log".to_string(),
            session_id: Some(session_id.to_string()),
            log: Some(log),
            ..Default::default()
        }),
        AgentEvent::Iteration { current, total: _ } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "iteration".to_string(),
            session_id: Some(session_id.to_string()),
            iteration: Some(current as i32),
            ..Default::default()
        }),
        AgentEvent::AssistantMessage { content } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "message_complete".to_string(),
            session_id: Some(session_id.to_string()),
            output: Some(
                content
                    .into_iter()
                    .filter_map(|block| match block {
                        crate::message::ContentBlock::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ..Default::default()
        }),
        AgentEvent::AgentSwitched {
            agent_id,
            agent_name,
            description,
        } => MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
            agent: Agent {
                id: agent_id,
                name: agent_name,
                description,
                ..Default::default()
            },
            messages: vec![],
        }),
    };

    GatewayMessage::new(request_id.to_string(), envelope)
}
