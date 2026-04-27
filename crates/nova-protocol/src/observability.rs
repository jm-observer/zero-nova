use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// --- Plan 1: Runtime Snapshots ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentInspectRequest {
    pub session_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentInspectResponse {
    pub agent_id: String,
    pub session_id: String,
    pub effective_model: ModelBindingDetailView,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeSnapshot {
    pub session_id: String,
    pub active_agent: String,
    pub model_override: SessionModelOverride,
    pub last_turn: Option<LastTurnSnapshot>,
    pub token_counters: SessionTokenCounters,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelOverride {
    pub orchestration: Option<ModelRef>,
    pub execution: Option<ModelRef>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct LastTurnSnapshot {
    pub turn_id: String,
    pub prepared_at: i64,
    pub prompt_preview: Option<PromptPreviewSnapshot>,
    pub tools: Vec<ToolAvailabilitySnapshot>,
    pub skills: Vec<SkillBindingSnapshot>,
    pub memory_hits: Vec<MemoryHitSnapshot>,
    pub usage: Option<TurnUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenCounters {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptPreviewRequest {
    pub session_id: String,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptPreviewSnapshot {
    pub system_prompt: String,
    pub tool_sections: Vec<String>,
    pub skill_sections: Vec<String>,
    pub conversation_summary: Option<String>,
    pub history_message_count: usize,
    pub active_skill: Option<String>,
    pub capability_policy_summary: Option<String>,
    pub max_tokens: Option<u32>,
    pub iteration_budget: Option<u32>,
    pub rendered_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionToolsRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionToolsResponse {
    pub tools: Vec<ToolAvailabilitySnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolAvailabilitySnapshot {
    pub name: String,
    pub source: String, // builtin, mcp_server, mcp_client, custom, skill_unlocked
    pub description: Option<String>,
    pub schema_summary: Value,
    pub enabled: bool,
    pub unlocked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionSkillBindingsRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionSkillBindingsResponse {
    pub skills: Vec<SkillBindingSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillBindingSnapshot {
    pub skill_id: String,
    pub name: String,
    pub status: String, // active, bound, available
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionMemoryHitsRequest {
    pub session_id: String,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionMemoryHitsResponse {
    pub hits: Vec<MemoryHitSnapshot>,
    pub enabled: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHitSnapshot {
    pub memory_id: String,
    pub title: String,
    pub score: f32,
    pub reason: Option<String>,
    pub excerpt: Option<String>,
    pub source: Option<String>,
    pub injected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelOverrideRequest {
    pub session_id: String,
    pub orchestration: Option<ModelRef>,
    pub execution: Option<ModelRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageResponse {
    pub usage: SessionTokenCounters,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelBindingDetailView {
    pub orchestration: ModelRef,
    pub execution: ModelRef,
    pub source: String, // global_default, agent_default, session_override
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

// --- Plan 2: Execution Records & Control ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionRunsRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionRunsResponse {
    pub runs: Vec<RunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunDetailRequest {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub agent_id: String,
    pub status: String, // queued, running, waiting_user, paused, stopped, failed, completed
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub orchestration_model: Option<ModelRef>,
    pub execution_model: Option<ModelRef>,
    pub usage: Option<TurnUsage>,
    pub error_summary: Option<String>,
    pub waiting_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunStepRecord {
    pub step_id: String,
    pub run_id: String,
    pub step_type: String, // thinking, tool, approval, message, artifact, system
    pub title: String,
    pub status: String,
    pub tool_name: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunControlRequest {
    pub run_id: String,
    pub action: String, // stop, resume_waiting, pause, resume, retry
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionArtifactsRequest {
    pub session_id: String,
    pub run_id: Option<String>,
    pub artifact_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionArtifactsResponse {
    pub artifacts: Vec<ArtifactRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub session_id: String,
    pub run_id: String,
    pub step_id: String,
    pub artifact_type: String,
    pub path: String,
    pub filename: String,
    pub content_preview: Option<String>,
    pub language: Option<String>,
    pub size: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPendingRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPendingResponse {
    pub requests: Vec<PermissionRequestRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRespondRequest {
    pub request_id: String,
    pub action: String,                 // approve, deny
    pub remember_scope: Option<String>, // session, permanent
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestRecord {
    pub request_id: String,
    pub session_id: String,
    pub run_id: String,
    pub step_id: String,
    pub agent_id: String,
    pub kind: String,
    pub title: String,
    pub reason: Option<String>,
    pub target: String,
    pub risk_level: String,
    pub status: String, // pending, approved, denied, expired
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogsRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogsResponse {
    pub logs: Vec<AuditLogRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogRecord {
    pub log_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub action: String,
    pub actor: String,
    pub detail: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsCurrentRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResponse {
    pub issues: Vec<DiagnosticIssueRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticIssueRecord {
    pub issue_id: String,
    pub category: String, // llm, mcp, memory, permission, protocol, artifact, runtime, unknown
    pub title: String,
    pub message: String,
    pub severity: String, // error, warning, info
    pub action_hint: Option<String>,
    pub count: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreRequest {
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreResponse {
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub console_visible: bool,
    pub active_tab: String,
    pub selected_run_id: Option<String>,
    pub selected_artifact_id: Option<String>,
    pub selected_permission_request_id: Option<String>,
    pub selected_diagnostic_id: Option<String>,
    pub restorable_run_state: String, // none, view_only, reattachable
    pub updated_at: i64,
}
