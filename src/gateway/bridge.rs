use crate::event::AgentEvent;
use crate::gateway::protocol::{ChatCompletePayload, ErrorPayload, GatewayMessage, MessageEnvelope, ProgressEvent};
use crate::message::ContentBlock;

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
            tool: Some(format!("{}:{}", name, id)),
            args: Some(input),
            ..Default::default()
        }),
        AgentEvent::ToolEnd {
            id: _,
            name: _,
            output,
            is_error: _,
        } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_result".to_string(),
            session_id: Some(session_id.to_string()),
            result: Some(output.into()),
            ..Default::default()
        }),
        AgentEvent::TurnComplete { new_messages, usage } => {
            // 获取最后一条消息作为输出
            let output = new_messages.last().and_then(|m| {
                m.content.first().and_then(|c| match c {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
            });
            MessageEnvelope::ChatComplete(ChatCompletePayload {
                session_id: session_id.to_string(),
                output,
                usage: Some(usage),
            })
        }
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
