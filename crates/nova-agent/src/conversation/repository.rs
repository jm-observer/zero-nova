use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteSessionRepository {
    pool: sqlx::SqlitePool,
}

impl SqliteSessionRepository {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save_session(
        &self,
        id: &str,
        title: &str,
        agent_id: &str,
        created_at: i64,
        updated_at: i64,
        runtime_control: &super::control::ControlState,
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
        runtime_control: &super::control::ControlState,
    ) -> Result<()> {
        let runtime_control_json = serde_json::to_string(runtime_control)?;
        sqlx::query("UPDATE sessions SET runtime_control = ?, updated_at = ? WHERE id = ?")
            .bind(runtime_control_json)
            .bind(chrono::Utc::now().timestamp_millis())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_message(
        &self,
        session_id: &str,
        role: Role,
        content: Vec<ContentBlock>,
        created_at: i64,
    ) -> Result<()> {
        let role_str = match role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let content_json = serde_json::to_string(&content)?;

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, created_at) 
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(role_str)
        .bind(content_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
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
            super::control::ControlState,
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
                serde_json::from_str(&json)?
            } else {
                super::control::ControlState::new(&agent_id)
            };

            let messages_rows =
                sqlx::query("SELECT role, content FROM messages WHERE session_id = ? ORDER BY created_at, id")
                    .bind(&id)
                    .fetch_all(&self.pool)
                    .await?;

            let mut history = Vec::new();
            for m_row in messages_rows {
                let role_str: String = m_row.get("role");
                let content_str: String = m_row.get("content");
                let role = match role_str.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    _ => Role::Assistant,
                };
                let content: Vec<ContentBlock> = serde_json::from_str(&content_str)?;
                history.push(Message { role, content });
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

    pub async fn list_sessions(&self) -> Result<Vec<(String, String, String, i64, i64, super::control::ControlState)>> {
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
                serde_json::from_str(&json).unwrap_or_else(|_| super::control::ControlState::new(&agent_id))
            } else {
                super::control::ControlState::new(&agent_id)
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

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // --- Plan 2: Runs & Steps ---

    pub async fn create_run(&self, run: &super::model::RunRecord) -> Result<()> {
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
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
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

    pub async fn create_run_step(&self, step: &super::model::RunStepRecord) -> Result<()> {
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

    // --- Plan 2: Artifacts ---

    pub async fn create_artifact(&self, artifact: &super::model::ArtifactRecord) -> Result<()> {
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

    pub async fn list_artifacts(&self, session_id: &str) -> Result<Vec<super::model::ArtifactRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, name, content_type, storage_path, created_at FROM artifacts WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut artifacts = Vec::new();
        for row in rows {
            artifacts.push(super::model::ArtifactRecord {
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

    // --- Plan 2: Permissions ---

    pub async fn create_permission_request(&self, req: &super::model::PermissionRequestRecord) -> Result<()> {
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

    // --- Plan 2: Diagnostics & Audit ---

    pub async fn create_audit_log(&self, log: &super::model::AuditLogRecord) -> Result<()> {
        let details_json = serde_json::to_string(&log.details).unwrap();
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

    pub async fn create_diagnostic_issue(&self, issue: &super::model::DiagnosticIssue) -> Result<()> {
        let details_json = issue.details.as_ref().map(|v| serde_json::to_string(v).unwrap());
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

    // --- Plan 2: Workspace Restore ---

    pub async fn save_workspace_restore_state(&self, state: &super::model::WorkspaceRestoreState) -> Result<()> {
        let snapshot_json = serde_json::to_string(&state.snapshot).unwrap();
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
    ) -> Result<Option<super::model::WorkspaceRestoreState>> {
        let row =
            sqlx::query("SELECT session_id, snapshot, updated_at FROM workspace_restore_state WHERE session_id = ?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(super::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_last_workspace_restore_state(&self) -> Result<Option<super::model::WorkspaceRestoreState>> {
        let row = sqlx::query(
            "SELECT session_id, snapshot, updated_at FROM workspace_restore_state ORDER BY updated_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(super::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_permission_requests(
        &self,
        session_id: &str,
    ) -> Result<Vec<super::model::PermissionRequestRecord>> {
        let rows = sqlx::query(
            "SELECT request_id, session_id, run_id, kind, target, status, reason, created_at FROM permission_requests WHERE session_id = ? ORDER BY created_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut requests = Vec::new();
        for row in rows {
            requests.push(super::model::PermissionRequestRecord {
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

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<Vec<super::model::AuditLogRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, action, details, created_at FROM audit_logs WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut logs = Vec::new();
        for row in rows {
            let details_json: String = row.get("details");
            logs.push(super::model::AuditLogRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                action: row.get("action"),
                details: serde_json::from_str(&details_json).unwrap_or(serde_json::Value::Null),
                created_at: row.get("created_at"),
            });
        }
        Ok(logs)
    }

    pub async fn list_diagnostics(&self, session_id: &str) -> Result<Vec<super::model::DiagnosticIssue>> {
        let rows = sqlx::query("SELECT id, session_id, severity, message, details, created_at FROM diagnostic_issues WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut issues = Vec::new();
        for row in rows {
            let details_json: Option<String> = row.get("details");
            issues.push(super::model::DiagnosticIssue {
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

    pub async fn list_runs(&self, session_id: &str) -> Result<Vec<super::model::RunRecord>> {
        let rows = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE session_id = ? ORDER BY started_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut runs = Vec::new();
        for row in rows {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            runs.push(super::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
            });
        }
        Ok(runs)
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<super::model::RunRecord>> {
        let row = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            Ok(Some(super::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
            }))
        } else {
            Ok(None)
        }
    }
}

fn parse_model_ref(raw: Option<String>) -> Result<Option<super::control::ModelRef>> {
    raw.map(|value| serde_json::from_str(&value).context("Failed to parse run model metadata"))
        .transpose()
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "cancelled" | "stopped")
}

fn is_terminal_step_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "cancelled" | "stopped")
}

#[cfg(test)]
mod tests {
    use super::SqliteSessionRepository;
    use crate::conversation::control::ControlState;
    use crate::conversation::model::{RunRecord, RunStepRecord};
    use crate::conversation::sqlite_manager::SqliteManager;
    use anyhow::Result;
    use serde_json::json;
    use sqlx::Row;
    use tempfile::tempdir;

    #[tokio::test]
    async fn permission_repository_matches_current_schema() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repo = SqliteSessionRepository::new(manager.pool.clone());

        repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
            .await?;

        repo.create_run(&RunRecord {
            id: "run-1".to_string(),
            session_id: "session-1".to_string(),
            status: "running".to_string(),
            created_at: 100,
            updated_at: 100,
            orchestration_model: Some(crate::conversation::control::ModelRef {
                provider: "default".to_string(),
                model: "gpt-4.1".to_string(),
            }),
            execution_model: Some(crate::conversation::control::ModelRef {
                provider: "default".to_string(),
                model: "gpt-4.1-mini".to_string(),
            }),
            tool_call_count: Some(0),
        })
        .await?;

        repo.create_permission_request(&crate::conversation::model::PermissionRequestRecord {
            id: "perm-1".to_string(),
            session_id: "session-1".to_string(),
            run_id: "run-1".to_string(),
            capability: "filesystem".to_string(),
            resource: "D:/tmp/file.txt".to_string(),
            status: "pending".to_string(),
            reason: Some("need access".to_string()),
            created_at: 100,
        })
        .await?;

        let pending = repo.list_permission_requests("session-1").await?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "perm-1");
        assert_eq!(pending[0].capability, "filesystem");
        assert_eq!(pending[0].resource, "D:/tmp/file.txt");
        assert_eq!(pending[0].status, "pending");

        repo.resolve_permission_request("perm-1", "allowed", None).await?;

        let rows = sqlx::query("SELECT request_id, status, resolved_at FROM permission_requests WHERE request_id = ?")
            .bind("perm-1")
            .fetch_all(&manager.pool)
            .await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<String, _>("request_id"), "perm-1");
        assert_eq!(rows[0].get::<String, _>("status"), "allowed");
        assert!(rows[0].get::<Option<i64>, _>("resolved_at").is_some());

        Ok(())
    }

    #[tokio::test]
    async fn run_repository_matches_current_schema() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repo = SqliteSessionRepository::new(manager.pool.clone());

        repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
            .await?;

        repo.create_run(&RunRecord {
            id: "run-1".to_string(),
            session_id: "session-1".to_string(),
            status: "running".to_string(),
            created_at: 100,
            updated_at: 100,
            orchestration_model: Some(crate::conversation::control::ModelRef {
                provider: "default".to_string(),
                model: "gpt-4.1".to_string(),
            }),
            execution_model: Some(crate::conversation::control::ModelRef {
                provider: "default".to_string(),
                model: "gpt-4.1-mini".to_string(),
            }),
            tool_call_count: Some(0),
        })
        .await?;

        repo.create_run_step(&RunStepRecord {
            id: "step-1".to_string(),
            run_id: "run-1".to_string(),
            step_type: "tool_use".to_string(),
            status: "running".to_string(),
            input: Some(json!({"x": 1})),
            output: None,
            created_at: 110,
            updated_at: 110,
        })
        .await?;

        repo.update_run_step("step-1", "success", Some(&json!({"ok": true})), 120)
            .await?;
        repo.update_run_status("run-1", "success", 130).await?;

        let run = repo.get_run("run-1").await?.expect("run should exist");
        assert_eq!(run.id, "run-1");
        assert_eq!(run.session_id, "session-1");
        assert_eq!(run.status, "success");
        assert_eq!(run.created_at, 100);
        assert_eq!(run.updated_at, 130);
        assert_eq!(
            run.orchestration_model.as_ref().map(|model| model.model.as_str()),
            Some("gpt-4.1")
        );
        assert_eq!(
            run.execution_model.as_ref().map(|model| model.model.as_str()),
            Some("gpt-4.1-mini")
        );
        assert_eq!(run.tool_call_count, Some(1));

        let runs = repo.list_runs("session-1").await?;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
        assert_eq!(runs[0].tool_call_count, Some(1));

        Ok(())
    }
}
