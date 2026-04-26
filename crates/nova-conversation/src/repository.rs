use anyhow::Result;
use nova_core::message::{ContentBlock, Role};
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
        runtime_control: &crate::control::ControlState,
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
        runtime_control: &crate::control::ControlState,
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
            crate::control::ControlState,
            Vec<nova_core::message::Message>,
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
                crate::control::ControlState::new(&agent_id)
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
                history.push(nova_core::message::Message { role, content });
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

    pub async fn list_sessions(&self) -> Result<Vec<(String, String, String, i64, i64, crate::control::ControlState)>> {
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
                serde_json::from_str(&json).unwrap_or_else(|_| crate::control::ControlState::new(&agent_id))
            } else {
                crate::control::ControlState::new(&agent_id)
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

    pub async fn create_run(&self, run: &crate::model::RunRecord) -> Result<()> {
        sqlx::query("INSERT INTO runs (id, session_id, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&run.id)
            .bind(&run.session_id)
            .bind(&run.status)
            .bind(run.created_at)
            .bind(run.updated_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_run_status(&self, id: &str, status: &str, now: i64) -> Result<()> {
        sqlx::query("UPDATE runs SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_run_step(&self, step: &crate::model::RunStepRecord) -> Result<()> {
        let input_json = step.input.as_ref().map(|v| serde_json::to_string(v).unwrap());
        let output_json = step.output.as_ref().map(|v| serde_json::to_string(v).unwrap());
        sqlx::query("INSERT INTO run_steps (id, run_id, step_type, status, input, output, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&step.id)
            .bind(&step.run_id)
            .bind(&step.step_type)
            .bind(&step.status)
            .bind(input_json)
            .bind(output_json)
            .bind(step.created_at)
            .bind(step.updated_at)
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
        let output_json = output.map(|v| serde_json::to_string(v).unwrap());
        sqlx::query("UPDATE run_steps SET status = ?, output = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(output_json)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Plan 2: Artifacts ---

    pub async fn create_artifact(&self, artifact: &crate::model::ArtifactRecord) -> Result<()> {
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

    pub async fn list_artifacts(&self, session_id: &str) -> Result<Vec<crate::model::ArtifactRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, name, content_type, storage_path, created_at FROM artifacts WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut artifacts = Vec::new();
        for row in rows {
            artifacts.push(crate::model::ArtifactRecord {
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

    pub async fn create_permission_request(&self, req: &crate::model::PermissionRequestRecord) -> Result<()> {
        sqlx::query("INSERT INTO permission_requests (id, session_id, run_id, capability, resource, status, reason, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&req.id)
            .bind(&req.session_id)
            .bind(&req.run_id)
            .bind(&req.capability)
            .bind(&req.resource)
            .bind(&req.status)
            .bind(&req.reason)
            .bind(req.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn resolve_permission_request(&self, id: &str, status: &str, reason: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE permission_requests SET status = ?, reason = ? WHERE id = ?")
            .bind(status)
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Plan 2: Diagnostics & Audit ---

    pub async fn create_audit_log(&self, log: &crate::model::AuditLogRecord) -> Result<()> {
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

    pub async fn create_diagnostic_issue(&self, issue: &crate::model::DiagnosticIssue) -> Result<()> {
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

    pub async fn save_workspace_restore_state(&self, state: &crate::model::WorkspaceRestoreState) -> Result<()> {
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
    ) -> Result<Option<crate::model::WorkspaceRestoreState>> {
        let row =
            sqlx::query("SELECT session_id, snapshot, updated_at FROM workspace_restore_state WHERE session_id = ?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(crate::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_last_workspace_restore_state(
        &self,
    ) -> Result<Option<crate::model::WorkspaceRestoreState>> {
        let row =
            sqlx::query("SELECT session_id, snapshot, updated_at FROM workspace_restore_state ORDER BY updated_at DESC LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;

        if let Some(row) = row {
            let snapshot_json: String = row.get("snapshot");
            Ok(Some(crate::model::WorkspaceRestoreState {
                session_id: row.get("session_id"),
                snapshot: serde_json::from_str(&snapshot_json)?,
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_permission_requests(&self, session_id: &str) -> Result<Vec<crate::model::PermissionRequestRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, capability, resource, status, reason, created_at FROM permission_requests WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        
        let mut requests = Vec::new();
        for row in rows {
            requests.push(crate::model::PermissionRequestRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                capability: row.get("capability"),
                resource: row.get("resource"),
                status: row.get("status"),
                reason: row.get("reason"),
                created_at: row.get("created_at"),
            });
        }
        Ok(requests)
    }

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<Vec<crate::model::AuditLogRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, action, details, created_at FROM audit_logs WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        
        let mut logs = Vec::new();
        for row in rows {
            let details_json: String = row.get("details");
            logs.push(crate::model::AuditLogRecord {
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

    pub async fn list_diagnostics(&self, session_id: &str) -> Result<Vec<crate::model::DiagnosticIssue>> {
        let rows = sqlx::query("SELECT id, session_id, severity, message, details, created_at FROM diagnostic_issues WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        
        let mut issues = Vec::new();
        for row in rows {
            let details_json: Option<String> = row.get("details");
            issues.push(crate::model::DiagnosticIssue {
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

    pub async fn list_runs(&self, session_id: &str) -> Result<Vec<crate::model::RunRecord>> {
        let rows = sqlx::query("SELECT id, session_id, status, created_at, updated_at FROM runs WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        
        let mut runs = Vec::new();
        for row in rows {
            runs.push(crate::model::RunRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }
        Ok(runs)
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<crate::model::RunRecord>> {
        let row = sqlx::query("SELECT id, session_id, status, created_at, updated_at FROM runs WHERE id = ?")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?;
        
        if let Some(row) = row {
            Ok(Some(crate::model::RunRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }
}
