use nova_agent::message::ContentBlock;
use nova_agent::prompt::SkillInvocationLevel;
use nova_agent::provider::types::Usage;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AppSession {
    pub id: String,
    pub title: Option<String>,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

#[derive(Debug, Clone)]
pub struct AppAgent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Token(String),
    ThinkingDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    ToolLog {
        id: String,
        name: String,
        log: String,
        stream: String,
    },
    Iteration {
        current: usize,
        total: usize,
    },
    IterationLimitReached {
        iterations: usize,
    },
    AssistantMessage {
        content: Vec<ContentBlock>,
    },
    TurnComplete {
        usage: Usage,
    },
    Error(String),
    SystemLog(String),
    AgentSwitched {
        agent: AppAgent,
    },
    Welcome {
        require_auth: bool,
        setup_required: bool,
    },
    TaskCreated {
        id: String,
        subject: String,
    },
    TaskStatusChanged {
        id: String,
        subject: String,
        status: String,
        active_form: Option<String>,
    },
    BackgroundTaskComplete {
        id: String,
        name: String,
    },
    SkillLoaded {
        skill_name: String,
    },
    SkillActivated {
        skill_id: String,
        skill_name: String,
        sticky: bool,
    },
    SkillSwitched {
        from_skill: String,
        to_skill: String,
    },
    SkillExited {
        skill_id: String,
    },
    SkillRouteEvaluated {
        confidence: f64,
        reasoning: String,
    },
    ToolUnlocked {
        tool_name: String,
    },
    SkillInvocation {
        skill_id: String,
        skill_name: String,
        level: SkillInvocationLevel,
    },
    // --- Observability & Control (Plan 1 & 2) ---
    SessionRuntimeUpdated(nova_protocol::observability::SessionRuntimeSnapshot),
    SessionTokenUsageUpdated(nova_protocol::observability::SessionTokenUsageResponse),
    SessionToolsUpdated(nova_protocol::observability::SessionToolsResponse),
    SessionSkillBindingsUpdated(nova_protocol::observability::SessionSkillBindingsResponse),
    SessionMemoryHit(nova_protocol::observability::MemoryHitSnapshot),
    RunStatusUpdated(nova_protocol::observability::RunRecord),
    RunStepUpdated(nova_protocol::observability::RunStepRecord),
    SessionArtifactsUpdated(nova_protocol::observability::ArtifactRecord),
    PermissionRequested(nova_protocol::observability::PermissionRequestRecord),
    PermissionResolved(nova_protocol::observability::PermissionRequestRecord),
    AuditLogsUpdated(nova_protocol::observability::AuditLogRecord),
    DiagnosticsUpdated(nova_protocol::observability::DiagnosticsResponse),
    WorkspaceRestoreAvailable(nova_protocol::observability::WorkspaceRestoreResponse),
}

impl From<nova_agent::event::AgentEvent> for AppEvent {
    fn from(event: nova_agent::event::AgentEvent) -> Self {
        match event {
            nova_agent::event::AgentEvent::TextDelta(text) => AppEvent::Token(text),
            nova_agent::event::AgentEvent::ThinkingDelta(text) => AppEvent::ThinkingDelta(text),
            nova_agent::event::AgentEvent::ToolStart { id, name, input } => AppEvent::ToolStart { id, name, input },
            nova_agent::event::AgentEvent::ToolEnd {
                id,
                name,
                output,
                is_error,
            } => AppEvent::ToolEnd {
                id,
                name,
                output,
                is_error,
            },
            nova_agent::event::AgentEvent::LogDelta { id, name, log, stream } => {
                AppEvent::ToolLog { id, name, log, stream }
            }
            nova_agent::event::AgentEvent::Iteration { current, total } => AppEvent::Iteration { current, total },
            nova_agent::event::AgentEvent::IterationLimitReached { iterations } => {
                AppEvent::IterationLimitReached { iterations }
            }
            nova_agent::event::AgentEvent::AssistantMessage { content } => AppEvent::AssistantMessage { content },
            nova_agent::event::AgentEvent::TurnComplete { usage, .. } => AppEvent::TurnComplete { usage },
            nova_agent::event::AgentEvent::Error(msg) => AppEvent::Error(msg),
            nova_agent::event::AgentEvent::SystemLog(msg) => AppEvent::SystemLog(msg),
            nova_agent::event::AgentEvent::AgentSwitched {
                agent_id,
                agent_name,
                description,
            } => AppEvent::AgentSwitched {
                agent: AppAgent {
                    id: agent_id,
                    name: agent_name,
                    description,
                },
            },
            nova_agent::event::AgentEvent::TaskCreated { id, subject } => AppEvent::TaskCreated { id, subject },
            nova_agent::event::AgentEvent::TaskStatusChanged {
                id,
                subject,
                status,
                active_form,
            } => AppEvent::TaskStatusChanged {
                id,
                subject,
                status,
                active_form,
            },
            nova_agent::event::AgentEvent::BackgroundTaskComplete { id, name } => {
                AppEvent::BackgroundTaskComplete { id, name }
            }
            nova_agent::event::AgentEvent::SkillLoaded { skill_name } => AppEvent::SkillLoaded { skill_name },
            nova_agent::event::AgentEvent::SkillActivated {
                skill_id,
                skill_name,
                sticky,
                ..
            } => AppEvent::SkillActivated {
                skill_id,
                skill_name,
                sticky,
            },
            nova_agent::event::AgentEvent::SkillSwitched {
                from_skill, to_skill, ..
            } => AppEvent::SkillSwitched { from_skill, to_skill },
            nova_agent::event::AgentEvent::SkillExited { skill_id, .. } => AppEvent::SkillExited { skill_id },
            nova_agent::event::AgentEvent::SkillRouteEvaluated {
                confidence, reasoning, ..
            } => AppEvent::SkillRouteEvaluated { confidence, reasoning },
            nova_agent::event::AgentEvent::ToolUnlocked { tool_name } => AppEvent::ToolUnlocked { tool_name },
            nova_agent::event::AgentEvent::SkillInvocation {
                skill_id,
                skill_name,
                level,
            } => AppEvent::SkillInvocation {
                skill_id,
                skill_name,
                level,
            },
        }
    }
}
