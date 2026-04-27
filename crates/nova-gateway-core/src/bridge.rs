use nova_agent::app::types::{AppAgent, AppEvent, AppMessage, AppSession};
use nova_agent::message::ContentBlock;
use nova_protocol::{
    Agent, AgentsSwitchResponse, ContentBlockDTO, ErrorPayload, GatewayMessage, MessageDTO, MessageEnvelope,
    ProgressEvent, Session as SessionProtocol, SkillActivatedPayload, SkillExitedPayload, SkillInvocationPayload,
    SkillRouteEvaluatedPayload, SkillSwitchedPayload, TaskStatusChangedPayload, ToolUnlockedPayload, WelcomePayload,
};

/// 将 AppEvent 转换为 GatewayMessage。
pub fn app_event_to_gateway(event: AppEvent, request_id: &str, session_id: &str) -> GatewayMessage {
    let envelope = match event {
        AppEvent::Token(text) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "token".to_string(),
            session_id: Some(session_id.to_string()),
            token: Some(text),
            ..Default::default()
        }),
        AppEvent::ThinkingDelta(text) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "thinking".to_string(),
            session_id: Some(session_id.to_string()),
            thinking: Some(text),
            ..Default::default()
        }),
        AppEvent::ToolStart { id, name, input } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_start".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name),
            tool_use_id: Some(id),
            args: Some(input),
            ..Default::default()
        }),
        AppEvent::ToolEnd {
            id,
            name,
            output,
            is_error,
        } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_result".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name),
            tool_use_id: Some(id),
            result: Some(output.into()),
            is_error: Some(is_error),
            ..Default::default()
        }),
        AppEvent::ToolLog { id, name, log, stream } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_log".to_string(),
            session_id: Some(session_id.to_string()),
            tool_name: Some(name),
            tool_use_id: Some(id),
            log: Some(log),
            stream: Some(stream),
            ..Default::default()
        }),
        AppEvent::Iteration { current, total: _ } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "iteration".to_string(),
            session_id: Some(session_id.to_string()),
            iteration: Some(current as i32),
            ..Default::default()
        }),
        AppEvent::IterationLimitReached { iterations } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "iteration_limit".to_string(),
            session_id: Some(session_id.to_string()),
            iteration: Some(iterations as i32),
            ..Default::default()
        }),
        AppEvent::TurnComplete { .. } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "turn_complete".to_string(),
            session_id: Some(session_id.to_string()),
            ..Default::default()
        }),
        AppEvent::Error(msg) => MessageEnvelope::Error(ErrorPayload {
            message: msg,
            code: Some("AGENT_RUNTIME_ERROR".to_string()),
        }),
        AppEvent::SystemLog(log) => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "system_log".to_string(),
            session_id: Some(session_id.to_string()),
            log: Some(log),
            ..Default::default()
        }),
        AppEvent::AssistantMessage { content } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "message_complete".to_string(),
            session_id: Some(session_id.to_string()),
            output: Some(
                content
                    .into_iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ..Default::default()
        }),
        AppEvent::AgentSwitched { agent } => MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
            agent: app_agent_to_protocol(agent),
            messages: vec![],
        }),
        AppEvent::Welcome {
            require_auth,
            setup_required,
        } => MessageEnvelope::Welcome(WelcomePayload {
            require_auth,
            setup_required,
        }),
        AppEvent::TaskCreated { id, subject } => {
            let payload = TaskStatusChangedPayload {
                task_id: id.clone(),
                task_subject: subject.clone(),
                status: "pending".to_string(),
                is_main_task: true,
                ..Default::default()
            };
            MessageEnvelope::TaskStatusChanged(payload)
        }
        AppEvent::TaskStatusChanged {
            id, status, subject, ..
        } => {
            let payload = TaskStatusChangedPayload {
                task_id: id,
                task_subject: subject,
                status,
                is_main_task: false,
                ..Default::default()
            };
            MessageEnvelope::TaskStatusChanged(payload)
        }
        AppEvent::BackgroundTaskComplete { name, .. } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_log".to_string(),
            session_id: Some(session_id.to_string()),
            log: Some(format!("Background task '{}' complete", name)),
            stream: Some("stdout".to_string()),
            ..Default::default()
        }),
        AppEvent::SkillLoaded { skill_name } => MessageEnvelope::ChatProgress(ProgressEvent {
            kind: "tool_log".to_string(),
            session_id: Some(session_id.to_string()),
            log: Some(format!("Skill loaded: {}", skill_name)),
            stream: Some("stdout".to_string()),
            ..Default::default()
        }),
        AppEvent::SkillActivated {
            skill_id,
            skill_name,
            sticky,
        } => {
            let payload = SkillActivatedPayload {
                session_id: Some(session_id.to_string()),
                skill_id,
                skill_name,
                sticky,
                reason: "manual".to_string(),
            };
            MessageEnvelope::SkillActivated(payload)
        }
        AppEvent::SkillSwitched { from_skill, to_skill } => {
            let payload = SkillSwitchedPayload {
                session_id: Some(session_id.to_string()),
                from_skill,
                to_skill,
                reason: "manual".to_string(),
            };
            MessageEnvelope::SkillSwitched(payload)
        }
        AppEvent::SkillExited { skill_id } => {
            let payload = SkillExitedPayload {
                session_id: Some(session_id.to_string()),
                skill_id,
                skill_name: String::new(),
                reason: "manual".to_string(),
            };
            MessageEnvelope::SkillExited(payload)
        }
        AppEvent::SkillRouteEvaluated { confidence, reasoning } => {
            let payload = SkillRouteEvaluatedPayload {
                session_id: Some(session_id.to_string()),
                skill_id: "unknown".to_string(),
                confidence,
                decision: "route".to_string(),
                reasoning,
            };
            MessageEnvelope::SkillRouteEvaluated(payload)
        }
        AppEvent::ToolUnlocked { tool_name } => {
            let payload = ToolUnlockedPayload {
                session_id: Some(session_id.to_string()),
                tool_name,
                source: "tool_search".to_string(),
            };
            MessageEnvelope::ToolUnlocked(payload)
        }
        AppEvent::SkillInvocation {
            skill_id,
            skill_name,
            level,
        } => {
            let payload = SkillInvocationPayload {
                session_id: Some(session_id.to_string()),
                skill_id,
                skill_name,
                level: format!("{:?}", level),
            };
            MessageEnvelope::SkillInvocation(payload)
        }
        AppEvent::SessionRuntimeUpdated(payload) => MessageEnvelope::SessionRuntimeUpdated(payload),
        AppEvent::SessionTokenUsageUpdated(payload) => MessageEnvelope::SessionTokenUsageUpdated(payload),
        AppEvent::SessionToolsUpdated(payload) => MessageEnvelope::SessionToolsUpdated(payload),
        AppEvent::SessionSkillBindingsUpdated(payload) => MessageEnvelope::SessionSkillBindingsUpdated(payload),
        AppEvent::SessionMemoryHit(payload) => MessageEnvelope::SessionMemoryHit(payload),
        AppEvent::RunStatusUpdated(payload) => MessageEnvelope::RunStatusUpdated(payload),
        AppEvent::RunStepUpdated(payload) => MessageEnvelope::RunStepUpdated(payload),
        AppEvent::SessionArtifactsUpdated(payload) => MessageEnvelope::SessionArtifactsUpdated(payload),
        AppEvent::PermissionRequested(payload) => MessageEnvelope::PermissionRequested(payload),
        AppEvent::PermissionResolved(payload) => MessageEnvelope::PermissionResolved(payload),
        AppEvent::AuditLogsUpdated(payload) => MessageEnvelope::AuditLogsUpdated(payload),
        AppEvent::DiagnosticsUpdated(payload) => MessageEnvelope::DiagnosticsUpdated(payload),
        AppEvent::WorkspaceRestoreAvailable(payload) => MessageEnvelope::WorkspaceRestoreAvailable(payload),
    };

    GatewayMessage::new(request_id.to_string(), envelope)
}

pub fn app_session_to_protocol(session: AppSession) -> SessionProtocol {
    SessionProtocol {
        id: session.id,
        title: session.title,
        agent_id: session.agent_id,
        created_at: session.created_at,
        updated_at: session.updated_at,
        message_count: session.message_count,
    }
}

pub fn app_agent_to_protocol(agent: AppAgent) -> Agent {
    Agent {
        id: agent.id,
        name: agent.name,
        description: agent.description,
        icon: None,
        system_prompt: None,
    }
}

pub fn app_message_to_protocol(message: AppMessage) -> MessageDTO {
    MessageDTO {
        role: message.role,
        content: message
            .content
            .into_iter()
            .map(|block| match block {
                ContentBlock::Text { text } => ContentBlockDTO::Text { text },
                ContentBlock::Thinking { thinking } => ContentBlockDTO::Thinking { thinking },
                ContentBlock::ToolUse { id, name, input } => ContentBlockDTO::ToolUse { id, name, input },
                ContentBlock::ToolResult {
                    tool_use_id,
                    output,
                    is_error,
                } => ContentBlockDTO::ToolResult {
                    tool_use_id,
                    content: output,
                    is_error,
                },
            })
            .collect(),
        timestamp: message.timestamp,
    }
}
