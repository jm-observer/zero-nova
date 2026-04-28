use super::control::ModelRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub status: String, // pending, running, success, failed, cancelled
    pub created_at: i64,
    pub updated_at: i64,
    pub orchestration_model: Option<ModelRef>,
    pub execution_model: Option<ModelRef>,
    pub tool_call_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStepRecord {
    pub id: String,
    pub run_id: String,
    pub step_type: String, // tool_use, reasoning, planning, output
    pub status: String,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub name: String,
    pub content_type: String,
    pub storage_path: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestRecord {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub capability: String,
    pub resource: String,
    pub status: String, // pending, allowed, denied
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogRecord {
    pub id: i64,
    pub session_id: String,
    pub run_id: Option<String>,
    pub action: String,
    pub details: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticIssue {
    pub id: String,
    pub session_id: String,
    pub severity: String, // error, warning, info
    pub message: String,
    pub details: Option<Value>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRestoreState {
    pub session_id: String,
    pub snapshot: Value,
    pub updated_at: i64,
}
