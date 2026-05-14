pub mod message_repo;
pub mod session_repo;

use anyhow::{Context, Result};
use log::warn;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteSessionRepository {
    pub(super) pool: sqlx::SqlitePool,
}

type SessionRow = (String, String, String, i64, i64, super::control::ControlState);

#[derive(Debug, Clone)]
pub struct SessionUsageAggregate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct UsageQualityCounts {
    pub total_turns: u32,
    pub turns_with_unknown_cache: u32,
    pub turns_with_missing_usage: u32,
}

fn parse_model_ref(raw: Option<String>) -> Result<Option<super::control::ModelRef>> {
    raw.map(|value| serde_json::from_str(&value).context("Failed to parse run model metadata"))
        .transpose()
}

fn parse_usage(raw: Option<String>) -> Result<Option<serde_json::Value>> {
    raw.map(|value| serde_json::from_str(&value).context("Failed to parse run usage metadata"))
        .transpose()
}

fn parse_session_row(row: sqlx::sqlite::SqliteRow) -> Result<SessionRow> {
    let agent_id: String = row.get("agent_id");
    let session_id: String = row.get("id");
    let runtime_control_json: Option<String> = row.get("runtime_control");
    let runtime_control = if let Some(json) = runtime_control_json {
        serde_json::from_str(&json).unwrap_or_else(|e| {
            warn!(
                "Failed to decode runtime_control for session '{}': {e} (agent: {})",
                session_id, agent_id
            );
            super::control::ControlState::new(&agent_id)
        })
    } else {
        super::control::ControlState::new(&agent_id)
    };

    Ok((
        session_id,
        row.get("title"),
        agent_id,
        row.get("created_at"),
        row.get("updated_at"),
        runtime_control,
    ))
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "cancelled" | "stopped")
}

fn is_terminal_step_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "cancelled" | "stopped")
}

#[cfg(test)]
mod tests;

impl SqliteSessionRepository {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}
