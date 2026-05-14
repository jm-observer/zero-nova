use super::{
    is_terminal_run_status, is_terminal_step_status, parse_model_ref, parse_session_row, parse_usage, SessionRow,
    SessionUsageAggregate, SqliteSessionRepository, UsageQualityCounts,
};
use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use log::warn;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn save_session(
        &self,
        id: &str,
        title: &str,
        agent_id: &str,
        created_at: i64,
        updated_at: i64,
        runtime_control: &crate::conversation::control::ControlState,
    ) -> Result<()> {
        let runtime_control_json = serde_json::to_string(runtime_control)?;
        sqlx::query(
            "INSERT INTO sessions (id, title, agent_id, created_at, updated_at, runtime_control)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title,
                agent_id=excluded.agent_id,
                updated_at=excluded.updated_at,
                runtime_control=excluded.runtime_control",
        )
        .bind(id)
        .bind(title)
        .bind(agent_id)
        .bind(created_at)
        .bind(updated_at)
        .bind(runtime_control_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_session_runtime_control(
        &self,
        id: &str,
        runtime_control: &crate::conversation::control::ControlState,
        updated_at: i64,
    ) -> Result<()> {
        let runtime_control_json = serde_json::to_string(runtime_control)?;
        sqlx::query("UPDATE sessions SET runtime_control = ?, updated_at = ? WHERE id = ?")
            .bind(runtime_control_json)
            .bind(updated_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn load_session_meta(&self, id: &str) -> Result<Option<SessionRow>> {
        let row = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session_row).transpose()
    }

    pub async fn load_session(
        &self,
        id: &str,
    ) -> Result<
        Option<(
            String,
            String,
            String,
            i64,
            i64,
            crate::conversation::control::ControlState,
            Vec<Message>,
        )>,
    > {
        let row = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let id: String = row.get("id");
            let title: String = row.get("title");
            let agent_id: String = row.get("agent_id");
            let created_at: i64 = row.get("created_at");
            let updated_at: i64 = row.get("updated_at");
            let runtime_control_json: Option<String> = row.get("runtime_control");

            let runtime_control = if let Some(json) = runtime_control_json {
                serde_json::from_str::<crate::conversation::control::ControlState>(&json)?
            } else {
                crate::conversation::control::ControlState::new(&agent_id)
            };

            let messages_rows = sqlx::query(
                "SELECT message_id, role, content, metadata, created_at FROM messages WHERE session_id = ? ORDER BY created_at, id",
            )
            .bind(&id)
            .fetch_all(&self.pool)
            .await?;

            let mut history = Vec::new();
            for m_row in messages_rows {
                let role_str: String = m_row.get("role");
                let content_str: String = m_row.get("content");
                let message_id: String = m_row.get("message_id");
                let metadata_str: Option<String> = m_row.get("metadata");
                let created_at: i64 = m_row.get("created_at");
                let role = match role_str.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    _ => Role::Assistant,
                };
                let content: Vec<ContentBlock> = serde_json::from_str(&content_str)?;
                let metadata = metadata_str
                    .as_deref()
                    .map(serde_json::from_str::<crate::message::MessageMetadata>)
                    .transpose()?;
                history.push(Message {
                    id: message_id,
                    role,
                    content,
                    created_at,
                    metadata,
                });
            }

            return Ok(Some((
                id,
                title,
                agent_id,
                created_at,
                updated_at,
                runtime_control,
                history,
            )));
        }

        Ok(None)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let rows = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control FROM sessions ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for row in rows {
            let agent_id: String = row.get("agent_id");
            let runtime_control_json: Option<String> = row.get("runtime_control");
            let runtime_control = if let Some(json) = runtime_control_json {
                serde_json::from_str(&json).unwrap_or_else(|e| {
                    warn!(
                        "Failed to decode runtime_control for session '{}': {} (agent: {})",
                        row.get::<String, _>("id"),
                        e,
                        agent_id
                    );
                    crate::conversation::control::ControlState::new(&agent_id)
                })
            } else {
                crate::conversation::control::ControlState::new(&agent_id)
            };

            sessions.push((
                row.get("id"),
                row.get("title"),
                agent_id,
                row.get("created_at"),
                row.get("updated_at"),
                runtime_control,
            ));
        }
        Ok(sessions)
    }

    pub async fn find_latest_session_by_agent(&self, agent_id: &str) -> Result<Option<SessionRow>> {
        let row = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control
             FROM sessions
             WHERE agent_id = ?
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session_row).transpose()
    }

    pub async fn touch_session(&self, id: &str, updated_at: i64) -> Result<()> {
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(updated_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        crate::conversation::storage::sqlite_tx::delete_session_with_messages(&self.pool, id).await
    }

    pub async fn create_run(&self, run: &crate::conversation::model::RunRecord) -> Result<()> {
        let agent_id: String = sqlx::query("SELECT agent_id FROM sessions WHERE id = ?")
            .bind(&run.session_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to load session agent_id for run record")?
            .get("agent_id");
        let orchestration_model = run
            .orchestration_model
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let execution_model = run.execution_model.as_ref().map(serde_json::to_string).transpose()?;

        let usage_json = run.usage.as_ref().map(serde_json::to_string).transpose()?;
        sqlx::query(
            "INSERT INTO runs (run_id, session_id, turn_id, agent_id, status, started_at, finished_at, duration_ms, orchestration_model, execution_model, usage, error_summary, waiting_reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.session_id)
        .bind(&run.id)
        .bind(agent_id)
        .bind(&run.status)
        .bind(run.created_at)
        .bind(if is_terminal_run_status(&run.status) { Some(run.updated_at) } else { None })
        .bind(if is_terminal_run_status(&run.status) { Some(run.updated_at - run.created_at) } else { None })
        .bind(orchestration_model)
        .bind(execution_model)
        .bind(usage_json)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run_usage(&self, run_id: &str, usage: &serde_json::Value) -> Result<()> {
        let usage_json = serde_json::to_string(usage)?;
        sqlx::query("UPDATE runs SET usage = ? WHERE run_id = ?")
            .bind(usage_json)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_run_status(&self, id: &str, status: &str, now: i64) -> Result<()> {
        sqlx::query(
            "UPDATE runs SET status = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END, duration_ms = CASE WHEN ? THEN (? - started_at) ELSE duration_ms END WHERE run_id = ?",
        )
        .bind(status)
        .bind(is_terminal_run_status(status))
        .bind(now)
        .bind(is_terminal_run_status(status))
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_run_step(&self, step: &crate::conversation::model::RunStepRecord) -> Result<()> {
        let payload_json = serde_json::to_string(&serde_json::json!({
            "input": step.input,
            "output": step.output,
        }))?;
        let title = step.step_type.clone();

        sqlx::query(
            "INSERT INTO run_steps (step_id, run_id, step_type, title, tool_name, status, started_at, finished_at, payload) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&step.id)
        .bind(&step.run_id)
        .bind(&step.step_type)
        .bind(title)
        .bind(Option::<String>::None)
        .bind(&step.status)
        .bind(step.created_at)
        .bind(if is_terminal_step_status(&step.status) { Some(step.updated_at) } else { None })
        .bind(payload_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run_step(
        &self,
        id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        now: i64,
    ) -> Result<()> {
        let existing_payload: Option<String> = sqlx::query("SELECT payload FROM run_steps WHERE step_id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.get("payload"));
        let mut payload = existing_payload
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(output) = output {
            payload["output"] = output.clone();
        }
        let payload_json = serde_json::to_string(&payload)?;

        sqlx::query(
            "UPDATE run_steps SET status = ?, payload = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE step_id = ?",
        )
        .bind(status)
        .bind(payload_json)
        .bind(is_terminal_step_status(status))
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_artifact(&self, artifact: &crate::conversation::model::ArtifactRecord) -> Result<()> {
        sqlx::query("INSERT INTO artifacts (id, session_id, run_id, name, content_type, storage_path, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(&artifact.id)
            .bind(&artifact.session_id)
            .bind(&artifact.run_id)
            .bind(&artifact.name)
            .bind(&artifact.content_type)
            .bind(&artifact.storage_path)
            .bind(artifact.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_artifacts(&self, session_id: &str) -> Result<Vec<crate::conversation::model::ArtifactRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, name, content_type, storage_path, created_at FROM artifacts WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut artifacts = Vec::new();
        for row in rows {
            artifacts.push(crate::conversation::model::ArtifactRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                name: row.get("name"),
                content_type: row.get("content_type"),
                storage_path: row.get("storage_path"),
                created_at: row.get("created_at"),
            });
        }
        Ok(artifacts)
    }

    pub async fn create_permission_request(
        &self,
        req: &crate::conversation::model::PermissionRequestRecord,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO permission_requests (request_id, session_id, run_id, step_id, agent_id, kind, title, reason, target, risk_level, status, created_at, resolved_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&req.id)
        .bind(&req.session_id)
        .bind(&req.run_id)
        .bind("")
        .bind("")
        .bind(&req.capability)
        .bind(&req.resource)
        .bind(&req.reason)
        .bind(&req.resource)
        .bind("unknown")
        .bind(&req.status)
        .bind(req.created_at)
        .bind(Option::<i64>::None)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn resolve_permission_request(&self, id: &str, status: &str, _reason: Option<&str>) -> Result<()> {
        let resolved_at = if matches!(status, "allowed" | "denied") {
            Some(chrono::Utc::now().timestamp_millis())
        } else {
            None
        };

        sqlx::query("UPDATE permission_requests SET status = ?, resolved_at = ? WHERE request_id = ?")
            .bind(status)
            .bind(resolved_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_audit_log(&self, log: &crate::conversation::model::AuditLogRecord) -> Result<()> {
        let details_json = serde_json::to_string(&log.details)
            .with_context(|| format!("Failed to serialize audit_log.details for '{}'", log.action))?;
        sqlx::query("INSERT INTO audit_logs (session_id, run_id, action, details, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&log.session_id)
            .bind(&log.run_id)
            .bind(&log.action)
            .bind(details_json)
            .bind(log.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_diagnostic_issue(&self, issue: &crate::conversation::model::DiagnosticIssue) -> Result<()> {
        let details_json = issue
            .details
            .as_ref()
            .map(|v| {
                serde_json::to_string(v)
                    .with_context(|| format!("Failed to serialize diagnostic.details for '{}'", issue.id))
            })
            .transpose()?;
        sqlx::query("INSERT INTO diagnostic_issues (id, session_id, severity, message, details, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&issue.id)
            .bind(&issue.session_id)
            .bind(&issue.severity)
            .bind(&issue.message)
            .bind(details_json)
            .bind(issue.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_diagnostics(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM diagnostic_issues WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_workspace_restore_state(
        &self,
        state: &crate::conversation::model::WorkspaceRestoreState,
    ) -> Result<()> {
        let snapshot_json = serde_json::to_string(&state.snapshot).with_context(|| {
            format!(
                "Failed to serialize workspace_restore snapshot for session '{}'",
                state.session_id
            )
        })?;
        sqlx::query("INSERT INTO workspace_restore_state (session_id, snapshot, updated_at) VALUES (?, ?, ?) ON CONFLICT(session_id) DO UPDATE SET snapshot=excluded.snapshot, updated_at=excluded.updated_at")
            .bind(&state.session_id)
            .bind(snapshot_json)
            .bind(state.updated_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_workspace_restore_state(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::conversation::model::WorkspaceRestoreState>> {
        let row =
            sqlx::query("SELECT session_id, snapshot, updated_at FROM workspace_restore_state WHERE session_id = ?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(crate::conversation::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str::<serde_json::Value>(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_last_workspace_restore_state(
        &self,
    ) -> Result<Option<crate::conversation::model::WorkspaceRestoreState>> {
        let row = sqlx::query(
            "SELECT session_id, snapshot, updated_at FROM workspace_restore_state ORDER BY updated_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(crate::conversation::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str::<serde_json::Value>(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_permission_requests(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::conversation::model::PermissionRequestRecord>> {
        let rows = sqlx::query(
            "SELECT request_id, session_id, run_id, kind, target, status, reason, created_at FROM permission_requests WHERE session_id = ? ORDER BY created_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut requests = Vec::new();
        for row in rows {
            requests.push(crate::conversation::model::PermissionRequestRecord {
                id: row.get("request_id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                capability: row.get("kind"),
                resource: row.get("target"),
                status: row.get("status"),
                reason: row.get("reason"),
                created_at: row.get("created_at"),
            });
        }
        Ok(requests)
    }

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<Vec<crate::conversation::model::AuditLogRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, action, details, created_at FROM audit_logs WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut logs = Vec::new();
        for row in rows {
            let details_json: String = row.get("details");
            let details: serde_json::Value = serde_json::from_str(&details_json).unwrap_or_else(|e| {
                warn!(
                    "Failed to decode audit log details for '{:?}' (session '{}', action '{}'): {e} — using null",
                    row.get::<String, _>("id"),
                    row.get::<String, _>("session_id"),
                    row.get::<String, _>("action")
                );
                serde_json::Value::Null
            });
            logs.push(crate::conversation::model::AuditLogRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                action: row.get("action"),
                details,
                created_at: row.get("created_at"),
            });
        }
        Ok(logs)
    }

    pub async fn list_diagnostics(&self, session_id: &str) -> Result<Vec<crate::conversation::model::DiagnosticIssue>> {
        let rows = sqlx::query("SELECT id, session_id, severity, message, details, created_at FROM diagnostic_issues WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut issues = Vec::new();
        for row in rows {
            let details_json: Option<String> = row.get("details");
            issues.push(crate::conversation::model::DiagnosticIssue {
                id: row.get("id"),
                session_id: row.get("session_id"),
                severity: row.get("severity"),
                message: row.get("message"),
                details: details_json.and_then(|j| serde_json::from_str(&j).ok()),
                created_at: row.get("created_at"),
            });
        }
        Ok(issues)
    }

    pub async fn list_runs(&self, session_id: &str) -> Result<Vec<crate::conversation::model::RunRecord>> {
        let rows = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, usage, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE session_id = ? ORDER BY started_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut runs = Vec::new();
        for row in rows {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            let usage = parse_usage(row.get("usage"))?;
            runs.push(crate::conversation::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
                usage,
            });
        }
        Ok(runs)
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<crate::conversation::model::RunRecord>> {
        let row = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, usage, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            let usage = parse_usage(row.get("usage"))?;
            Ok(Some(crate::conversation::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
                usage,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn sum_session_usage(&self, session_id: &str) -> Result<SessionUsageAggregate> {
        let row = sqlx::query(
            "SELECT
                COALESCE(SUM(json_extract(usage, '$.inputTokens')), 0) AS input_tokens,
                COALESCE(SUM(json_extract(usage, '$.outputTokens')), 0) AS output_tokens,
                COALESCE(SUM(json_extract(usage, '$.cacheCreationInputTokens')), 0) AS cache_creation_input_tokens,
                COALESCE(SUM(json_extract(usage, '$.cacheReadInputTokens')), 0) AS cache_read_input_tokens
             FROM runs WHERE session_id = ? AND usage IS NOT NULL",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(SessionUsageAggregate {
            input_tokens: row.get::<i64, _>("input_tokens") as u64,
            output_tokens: row.get::<i64, _>("output_tokens") as u64,
            cache_creation_input_tokens: row.get::<i64, _>("cache_creation_input_tokens") as u64,
            cache_read_input_tokens: row.get::<i64, _>("cache_read_input_tokens") as u64,
        })
    }

    pub async fn count_usage_quality(&self, session_id: &str) -> Result<UsageQualityCounts> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) AS total_turns,
                COUNT(CASE WHEN usage IS NOT NULL
                    AND json_extract(usage, '$.cacheCreationInputTokens') IS NULL
                    AND json_extract(usage, '$.cacheReadInputTokens') IS NULL
                    THEN 1 END) AS turns_with_unknown_cache,
                COUNT(CASE WHEN usage IS NULL THEN 1 END) AS turns_with_missing_usage
             FROM runs WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(UsageQualityCounts {
            total_turns: row.get::<i64, _>("total_turns") as u32,
            turns_with_unknown_cache: row.get::<i64, _>("turns_with_unknown_cache") as u32,
            turns_with_missing_usage: row.get::<i64, _>("turns_with_missing_usage") as u32,
        })
    }
}
