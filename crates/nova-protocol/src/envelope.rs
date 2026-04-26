use crate::agent::{AgentsListResponse, AgentsSwitchResponse, SessionAgentSwitchPayload};
use crate::chat::{
    ChatCompletePayload, ChatIntentPayload, ChatPayload, ProgressEvent, SkillActivatedPayload, SkillExitedPayload,
    SkillInvocationPayload, SkillRouteEvaluatedPayload, SkillSwitchedPayload, TaskStatusChangedPayload,
    ToolUnlockedPayload,
};
use crate::session::{
    SessionCopyRequest, SessionCreateRequest, SessionCreateResponse, SessionIdPayload, SessionsListResponse,
    SessionsMessagesResponse, SuccessResponse,
};
use crate::system::{ErrorPayload, WelcomePayload};
use serde::{Deserialize, Serialize};

/// 统一消息信封 (与 OpenFlux 前端规范完全一致)
/// 序列化结构: { "id": "...", "type": "...", "payload": { ... } }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// 请求响应模式下的消息 ID，事件推送时为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}

impl GatewayMessage {
    /// 创建一个新的请求或响应消息
    pub fn new(id: String, envelope: MessageEnvelope) -> Self {
        Self { id: Some(id), envelope }
    }

    /// 创建一个服务器主动推送的事件消息
    pub fn new_event(envelope: MessageEnvelope) -> Self {
        Self { id: None, envelope }
    }
}

/// 消息封装，包含类型标签和可选的 Payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum MessageEnvelope {
    // --- System & Control Events ---
    #[serde(rename = "error")]
    Error(ErrorPayload),
    #[serde(rename = "welcome")]
    Welcome(WelcomePayload),

    // --- Chat & Session Management ---
    #[serde(rename = "chat")]
    Chat(ChatPayload),
    #[serde(rename = "chat.stop")]
    ChatStop(SessionIdPayload),
    #[serde(rename = "chat.stop.response")]
    ChatStopResponse(SessionIdPayload),
    #[serde(rename = "sessions.list")]
    SessionsList,
    #[serde(rename = "sessions.list.response")]
    SessionsListResponse(SessionsListResponse),
    #[serde(rename = "sessions.messages")]
    SessionsMessages(SessionIdPayload),
    #[serde(rename = "sessions.messages.response")]
    SessionsMessagesResponse(SessionsMessagesResponse),
    #[serde(rename = "sessions.create")]
    SessionsCreate(SessionCreateRequest),
    #[serde(rename = "sessions.create.response")]
    SessionsCreateResponse(SessionCreateResponse),
    #[serde(rename = "sessions.delete")]
    SessionsDelete(SessionIdPayload),
    #[serde(rename = "sessions.delete.response")]
    SessionsDeleteResponse(SuccessResponse),
    #[serde(rename = "sessions.copy")]
    SessionsCopy(SessionCopyRequest),
    #[serde(rename = "sessions.copy.response")]
    SessionsCopyResponse(SessionCreateResponse),

    // --- Progress & Chat Events ---
    #[serde(rename = "chat.start")]
    ChatStart(SessionIdPayload),
    #[serde(rename = "chat.intent")]
    ChatIntent(ChatIntentPayload),
    #[serde(rename = "chat.progress")]
    ChatProgress(ProgressEvent),
    #[serde(rename = "chat.complete")]
    ChatComplete(ChatCompletePayload),

    // --- Agent Management ---
    #[serde(rename = "agents.list")]
    AgentsList,
    #[serde(rename = "agents.list.response")]
    AgentsListResponse(AgentsListResponse),
    #[serde(rename = "agents.switch")]
    AgentsSwitch(SessionAgentSwitchPayload),
    #[serde(rename = "agents.switch.response")]
    AgentsSwitchResponse(AgentsSwitchResponse),

    // --- System & Integration ---
    #[serde(rename = "config.get")]
    ConfigGet,
    #[serde(rename = "config.get.response")]
    ConfigGetResponse(serde_json::Value),
    #[serde(rename = "config.update")]
    ConfigUpdate(serde_json::Value),
    #[serde(rename = "config.update.response")]
    ConfigUpdateResponse(SuccessResponse),

    // --- Skill Events (Plan 4) ---
    #[serde(rename = "skill.activated")]
    SkillActivated(SkillActivatedPayload),
    #[serde(rename = "skill.switched")]
    SkillSwitched(SkillSwitchedPayload),
    #[serde(rename = "skill.exited")]
    SkillExited(SkillExitedPayload),
    #[serde(rename = "tool.unlocked")]
    ToolUnlocked(ToolUnlockedPayload),
    #[serde(rename = "task.status_changed")]
    TaskStatusChanged(TaskStatusChangedPayload),
    #[serde(rename = "skill.route_evaluated")]
    SkillRouteEvaluated(SkillRouteEvaluatedPayload),
    #[serde(rename = "skill.invocation")]
    SkillInvocation(SkillInvocationPayload),

    // --- Observability & Control (Plan 1 & 2) ---
    #[serde(rename = "agent.inspect")]
    AgentInspect(crate::observability::AgentInspectRequest),
    #[serde(rename = "agent.inspect.response")]
    AgentInspectResponse(crate::observability::AgentInspectResponse),
    #[serde(rename = "session.runtime")]
    SessionRuntime(crate::observability::SessionRuntimeRequest),
    #[serde(rename = "session.runtime.response")]
    SessionRuntimeResponse(crate::observability::SessionRuntimeSnapshot),
    #[serde(rename = "session.prompt.preview")]
    SessionPromptPreview(crate::observability::PromptPreviewRequest),
    #[serde(rename = "session.prompt.preview.response")]
    SessionPromptPreviewResponse(crate::observability::PromptPreviewSnapshot),
    #[serde(rename = "session.tools.list")]
    SessionToolsList(crate::observability::SessionToolsRequest),
    #[serde(rename = "session.tools.list.response")]
    SessionToolsListResponse(crate::observability::SessionToolsResponse),
    #[serde(rename = "session.skill.bindings")]
    SessionSkillBindings(crate::observability::SessionSkillBindingsRequest),
    #[serde(rename = "session.skill.bindings.response")]
    SessionSkillBindingsResponse(crate::observability::SessionSkillBindingsResponse),
    #[serde(rename = "session.memory.hits")]
    SessionMemoryHits(crate::observability::SessionMemoryHitsRequest),
    #[serde(rename = "session.memory.hits.response")]
    SessionMemoryHitsResponse(crate::observability::SessionMemoryHitsResponse),
    #[serde(rename = "session.model.override")]
    SessionModelOverride(crate::observability::SessionModelOverrideRequest),
    #[serde(rename = "session.model.override.response")]
    SessionModelOverrideResponse(crate::observability::SessionRuntimeSnapshot),
    #[serde(rename = "sessions.token_usage")]
    SessionTokenUsage(crate::observability::SessionTokenUsageRequest),
    #[serde(rename = "sessions.token_usage.response")]
    SessionTokenUsageResponse(crate::observability::SessionTokenUsageResponse),

    #[serde(rename = "session.runs")]
    SessionRuns(crate::observability::SessionRunsRequest),
    #[serde(rename = "session.runs.response")]
    SessionRunsResponse(crate::observability::SessionRunsResponse),
    #[serde(rename = "run.detail")]
    RunDetail(crate::observability::RunDetailRequest),
    #[serde(rename = "run.detail.response")]
    RunDetailResponse(crate::observability::RunRecord),
    #[serde(rename = "run.control")]
    RunControl(crate::observability::RunControlRequest),
    #[serde(rename = "run.control.response")]
    RunControlResponse(crate::session::SuccessResponse),
    #[serde(rename = "session.artifacts")]
    SessionArtifacts(crate::observability::SessionArtifactsRequest),
    #[serde(rename = "session.artifacts.response")]
    SessionArtifactsResponse(crate::observability::SessionArtifactsResponse),
    #[serde(rename = "permission.pending")]
    PermissionPending(crate::observability::PermissionPendingRequest),
    #[serde(rename = "permission.pending.response")]
    PermissionPendingResponse(crate::observability::PermissionPendingResponse),
    #[serde(rename = "permission.respond")]
    PermissionRespond(crate::observability::PermissionRespondRequest),
    #[serde(rename = "permission.respond.response")]
    PermissionRespondResponse(crate::session::SuccessResponse),
    #[serde(rename = "audit.logs")]
    AuditLogs(crate::observability::AuditLogsRequest),
    #[serde(rename = "audit.logs.response")]
    AuditLogsResponse(crate::observability::AuditLogsResponse),
    #[serde(rename = "diagnostics.current")]
    DiagnosticsCurrent(crate::observability::DiagnosticsCurrentRequest),
    #[serde(rename = "diagnostics.current.response")]
    DiagnosticsResponse(crate::observability::DiagnosticsResponse),
    #[serde(rename = "workspace.restore")]
    WorkspaceRestore(crate::observability::WorkspaceRestoreRequest),
    #[serde(rename = "workspace.restore.response")]
    WorkspaceRestoreResponse(crate::observability::WorkspaceRestoreResponse),

    // --- Events ---
    #[serde(rename = "session.runtime.updated")]
    SessionRuntimeUpdated(crate::observability::SessionRuntimeSnapshot),
    #[serde(rename = "session.token.usage")]
    SessionTokenUsageUpdated(crate::observability::SessionTokenUsageResponse),
    #[serde(rename = "session.tools.updated")]
    SessionToolsUpdated(crate::observability::SessionToolsResponse),
    #[serde(rename = "session.skill.bindings.updated")]
    SessionSkillBindingsUpdated(crate::observability::SessionSkillBindingsResponse),
    #[serde(rename = "session.memory.hit")]
    SessionMemoryHit(crate::observability::MemoryHitSnapshot),
    #[serde(rename = "run.status.updated")]
    RunStatusUpdated(crate::observability::RunRecord),
    #[serde(rename = "run.step.updated")]
    RunStepUpdated(crate::observability::RunStepRecord),
    #[serde(rename = "session.artifacts.updated")]
    SessionArtifactsUpdated(crate::observability::ArtifactRecord),
    #[serde(rename = "permission.requested")]
    PermissionRequested(crate::observability::PermissionRequestRecord),
    #[serde(rename = "permission.resolved")]
    PermissionResolved(crate::observability::PermissionRequestRecord),
    #[serde(rename = "audit.logs.updated")]
    AuditLogsUpdated(crate::observability::AuditLogRecord),
    #[serde(rename = "diagnostics.updated")]
    DiagnosticsUpdated(crate::observability::DiagnosticsResponse),
    #[serde(rename = "workspace.restore.available")]
    WorkspaceRestoreAvailable(crate::observability::WorkspaceRestoreResponse),

    #[serde(other)]
    Unknown,
}
