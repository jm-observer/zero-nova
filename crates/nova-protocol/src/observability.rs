use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealthRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealthSnapshot {
    pub provider: String,
    // ?
    pub scope: String,
    pub status: String,
    pub checked_at: i64,
    pub latency_ms: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealthSnapshotResponse {
    pub providers: Vec<ProviderHealthSnapshot>,
    pub updated_at: i64,
}

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
    #[serde(default)]
    pub system_prompt_state: SessionSystemPromptState,
    pub last_turn: Option<LastTurnSnapshot>,
    pub token_counters: SessionTokenCounters,
    #[serde(default)]
    pub loop_guard_metrics: LoopGuardMetrics,
    pub project_dir: Option<String>,
    pub project_dir_source: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoopGuardMetrics {
    pub total_triggers: u64,
    pub duplicate_tool_calls: u64,
    pub stalled_iterations: u64,
    pub rejected_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionSystemPromptState {
    pub version: String,
    pub updated_at: i64,
    pub source_revision: String,
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
pub struct SessionSystemPromptReloadRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionSystemPromptReloadResponse {
    pub session_id: String,
    pub version_before: String,
    pub version_after: String,
    pub updated_at: i64,
    pub changed: bool,
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
pub struct SessionFileTreeRequest {
    pub session_id: String,
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileTreeEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileTreeResponse {
    pub entries: Vec<SessionFileTreeEntry>,
    pub base_relative_path: String,
    pub project_dir_present: bool,
    pub updated_at: i64,
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
    pub source: String,
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
    #[serde(rename = "skillId", alias = "skill_id")]
    pub skill_id: String,
    pub name: String,
    pub status: String,
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
    pub summary: SessionTokenUsageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    #[default]
    Provider,
    Estimated,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageCompleteness {
    Full,
    #[default]
    Partial,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelBindingDetailView {
    pub orchestration: ModelRef,
    pub execution: ModelRef,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub source: UsageSource,
    #[serde(default)]
    pub completeness: UsageCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_provider_usage: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub total_turn_count: u32,
    pub turns_with_unknown_cache_usage: u32,
    pub turns_with_missing_usage: u32,
    pub last_turn_usage: Option<TurnUsage>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageDetailRequest {
    pub session_id: String,
    #[serde(default = "default_usage_detail_limit")]
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageDetailResponse {
    pub session_id: String,
    pub turns: Vec<TurnUsageDetail>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsageDetail {
    pub turn_id: String,
    pub run_id: String,
    pub status: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub usage: Option<TurnUsage>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

fn default_usage_detail_limit() -> u32 {
    20
}

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
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub orchestration_model: Option<ModelRef>,
    pub execution_model: Option<ModelRef>,
    pub tool_call_count: Option<u32>,
    pub usage: Option<TurnUsage>,
    pub error_summary: Option<String>,
    pub waiting_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunStepRecord {
    pub step_id: String,
    pub run_id: String,
    pub step_type: String,
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
    pub action: String,
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
    pub action: String,
    pub remember_scope: Option<String>,
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
    pub status: String,
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
    pub category: String,
    pub title: String,
    pub message: String,
    pub severity: String,
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
    pub restorable_run_state: String,
    pub updated_at: i64,
}
