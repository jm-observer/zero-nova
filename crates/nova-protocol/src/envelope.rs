use crate::agent::{AgentsListResponse, AgentsSwitchResponse, SessionAgentSwitchPayload};
use crate::chat::{
    ChatCompletePayload, ChatIntentPayload, ChatPayload, ProgressEvent, SkillActivatedPayload, SkillExitedPayload,
    SkillInvocationPayload, SkillRouteEvaluatedPayload, SkillSwitchedPayload, TaskStatusChangedPayload,
    ToolUnlockedPayload,
};
use crate::observability as obs;
use crate::session::{
    SessionCopyRequest, SessionCreateRequest, SessionCreateResponse, SessionIdPayload, SessionsListResponse,
    SessionsMessagesResponse, SuccessResponse,
};
use crate::system::{ErrorPayload, WelcomePayload};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 统一消息信封 (与 OpenFlux 前端规范完全一致)
/// 序列化结构: { "id": "...", "type": "...", "payload": { ... } }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    ConfigGetResponse(Value),
    #[serde(rename = "config.update")]
    ConfigUpdate(Value),
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
    AgentInspect(obs::AgentInspectRequest),
    #[serde(rename = "agent.inspect.response")]
    AgentInspectResponse(obs::AgentInspectResponse),
    #[serde(rename = "session.runtime")]
    SessionRuntime(obs::SessionRuntimeRequest),
    #[serde(rename = "session.runtime.response")]
    SessionRuntimeResponse(obs::SessionRuntimeSnapshot),
    #[serde(rename = "session.prompt.preview")]
    SessionPromptPreview(obs::PromptPreviewRequest),
    #[serde(rename = "session.prompt.preview.response")]
    SessionPromptPreviewResponse(obs::PromptPreviewSnapshot),
    #[serde(rename = "session.tools.list")]
    SessionToolsList(obs::SessionToolsRequest),
    #[serde(rename = "session.tools.list.response")]
    SessionToolsListResponse(obs::SessionToolsResponse),
    #[serde(rename = "session.skill.bindings")]
    SessionSkillBindings(obs::SessionSkillBindingsRequest),
    #[serde(rename = "session.skill.bindings.response")]
    SessionSkillBindingsResponse(obs::SessionSkillBindingsResponse),
    #[serde(rename = "session.memory.hits")]
    SessionMemoryHits(obs::SessionMemoryHitsRequest),
    #[serde(rename = "session.memory.hits.response")]
    SessionMemoryHitsResponse(obs::SessionMemoryHitsResponse),
    #[serde(rename = "session.model.override")]
    SessionModelOverride(obs::SessionModelOverrideRequest),
    #[serde(rename = "session.model.override.response")]
    SessionModelOverrideResponse(obs::SessionRuntimeSnapshot),
    #[serde(rename = "sessions.token_usage")]
    SessionTokenUsage(obs::SessionTokenUsageRequest),
    #[serde(rename = "sessions.token_usage.response")]
    SessionTokenUsageResponse(obs::SessionTokenUsageResponse),

    #[serde(rename = "session.runs")]
    SessionRuns(obs::SessionRunsRequest),
    #[serde(rename = "session.runs.response")]
    SessionRunsResponse(obs::SessionRunsResponse),
    #[serde(rename = "run.detail")]
    RunDetail(obs::RunDetailRequest),
    #[serde(rename = "run.detail.response")]
    RunDetailResponse(obs::RunRecord),
    #[serde(rename = "run.control")]
    RunControl(obs::RunControlRequest),
    #[serde(rename = "run.control.response")]
    RunControlResponse(SuccessResponse),
    #[serde(rename = "session.artifacts")]
    SessionArtifacts(obs::SessionArtifactsRequest),
    #[serde(rename = "session.artifacts.response")]
    SessionArtifactsResponse(obs::SessionArtifactsResponse),
    #[serde(rename = "permission.pending")]
    PermissionPending(obs::PermissionPendingRequest),
    #[serde(rename = "permission.pending.response")]
    PermissionPendingResponse(obs::PermissionPendingResponse),
    #[serde(rename = "permission.respond")]
    PermissionRespond(obs::PermissionRespondRequest),
    #[serde(rename = "permission.respond.response")]
    PermissionRespondResponse(SuccessResponse),
    #[serde(rename = "audit.logs")]
    AuditLogs(obs::AuditLogsRequest),
    #[serde(rename = "audit.logs.response")]
    AuditLogsResponse(obs::AuditLogsResponse),
    #[serde(rename = "diagnostics.current")]
    DiagnosticsCurrent(obs::DiagnosticsCurrentRequest),
    #[serde(rename = "diagnostics.current.response")]
    DiagnosticsResponse(obs::DiagnosticsResponse),
    #[serde(rename = "workspace.restore")]
    WorkspaceRestore(obs::WorkspaceRestoreRequest),
    #[serde(rename = "workspace.restore.response")]
    WorkspaceRestoreResponse(obs::WorkspaceRestoreResponse),

    // --- Events ---
    #[serde(rename = "session.runtime.updated")]
    SessionRuntimeUpdated(obs::SessionRuntimeSnapshot),
    #[serde(rename = "session.token.usage")]
    SessionTokenUsageUpdated(obs::SessionTokenUsageResponse),
    #[serde(rename = "session.tools.updated")]
    SessionToolsUpdated(obs::SessionToolsResponse),
    #[serde(rename = "session.skill.bindings.updated")]
    SessionSkillBindingsUpdated(obs::SessionSkillBindingsResponse),
    #[serde(rename = "session.memory.hit")]
    SessionMemoryHit(obs::MemoryHitSnapshot),
    #[serde(rename = "run.status.updated")]
    RunStatusUpdated(obs::RunRecord),
    #[serde(rename = "run.step.updated")]
    RunStepUpdated(obs::RunStepRecord),
    #[serde(rename = "session.artifacts.updated")]
    SessionArtifactsUpdated(obs::ArtifactRecord),
    #[serde(rename = "permission.requested")]
    PermissionRequested(obs::PermissionRequestRecord),
    #[serde(rename = "permission.resolved")]
    PermissionResolved(obs::PermissionRequestRecord),
    #[serde(rename = "audit.logs.updated")]
    AuditLogsUpdated(obs::AuditLogRecord),
    #[serde(rename = "diagnostics.updated")]
    DiagnosticsUpdated(obs::DiagnosticsResponse),
    #[serde(rename = "workspace.restore.available")]
    WorkspaceRestoreAvailable(obs::WorkspaceRestoreResponse),

    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}
