use anyhow::{Context, Result};
use log::warn;
use serde_json::json;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::path::Path;
use tokio::fs;

pub struct SqliteManager {
    pub pool: SqlitePool,
    pub db_path: String,
}

impl SqliteManager {
    pub async fn new(data_path: &Path) -> Result<Self> {
        if !data_path.exists() {
            fs::create_dir_all(data_path)
                .await
                .context("Failed to create data directory")?;
        }

        let db_path = data_path
            .join("sessions.db")
            .to_str()
            .context("SQLite database path contains non-UTF8 characters")?
            .to_string();

        let options = SqliteConnectOptions::new().filename(&db_path).create_if_missing(true);

        let pool = SqlitePool::connect_with(options)
            .await
            .context("Failed to connect to SQLite")?;

        let manager = Self { pool, db_path };

        manager.run_migrations().await?;

        Ok(manager)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                agent_id TEXT,
                created_at INTEGER,
                updated_at INTEGER,
                runtime_control TEXT -- Plan 1: Session extension
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create sessions table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create messages table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS runs (
                run_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                duration_ms INTEGER,
                orchestration_model TEXT,
                execution_model TEXT,
                usage TEXT,
                error_summary TEXT,
                waiting_reason TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create runs table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS run_steps (
                step_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                step_type TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                tool_name TEXT,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                payload TEXT,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create run_steps table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS artifacts (
                artifact_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                artifact_type TEXT NOT NULL,
                path TEXT NOT NULL,
                filename TEXT NOT NULL,
                content_preview TEXT,
                language TEXT,
                size INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create artifacts table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS permission_requests (
                request_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                reason TEXT,
                target TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                resolved_at INTEGER,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create permission_requests table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS audit_logs (
                log_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                run_id TEXT,
                action TEXT NOT NULL,
                actor TEXT NOT NULL,
                detail TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create audit_logs table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS diagnostic_issues (
                issue_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                category TEXT NOT NULL,
                title TEXT NOT NULL,
                message TEXT NOT NULL,
                severity TEXT NOT NULL,
                action_hint TEXT,
                count INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create diagnostic_issues table")?;

        self.create_workspace_restore_state_table().await?;
        self.migrate_sessions_runtime_control_column().await?;
        self.migrate_messages_timestamp_column().await?;
        self.migrate_workspace_restore_state_schema().await?;

        Ok(())
    }

    async fn create_workspace_restore_state_table(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS workspace_restore_state (
                session_id TEXT PRIMARY KEY,
                snapshot TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create workspace_restore_state table")?;

        Ok(())
    }

    async fn migrate_sessions_runtime_control_column(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(sessions)")
            .fetch_all(&self.pool)
            .await
            .context("Failed to inspect sessions table schema")?;

        let mut has_runtime_control = false;

        for column in columns {
            let name: String = Row::get(&column, "name");
            if name == "runtime_control" {
                has_runtime_control = true;
                break;
            }
        }

        if !has_runtime_control {
            sqlx::query("ALTER TABLE sessions ADD COLUMN runtime_control TEXT")
                .execute(&self.pool)
                .await
                .context("Failed to add runtime_control column to sessions table")?;
        }

        Ok(())
    }

    async fn migrate_messages_timestamp_column(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(messages)")
            .fetch_all(&self.pool)
            .await
            .context("Failed to inspect messages table schema")?;

        let mut has_created_at = false;
        let mut has_timestamp = false;

        for column in columns {
            let name: String = Row::get(&column, "name");
            if name == "created_at" {
                has_created_at = true;
            } else if name == "timestamp" {
                has_timestamp = true;
            }
        }

        if !has_created_at && has_timestamp {
            sqlx::query("ALTER TABLE messages ADD COLUMN created_at INTEGER")
                .execute(&self.pool)
                .await
                .context("Failed to add created_at column to messages table")?;

            sqlx::query("UPDATE messages SET created_at = timestamp WHERE created_at IS NULL")
                .execute(&self.pool)
                .await
                .context("Failed to backfill created_at from timestamp")?;
        }

        Ok(())
    }

    async fn migrate_workspace_restore_state_schema(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(workspace_restore_state)")
            .fetch_all(&self.pool)
            .await
            .context("Failed to inspect workspace_restore_state table schema")?;

        let column_names = columns
            .iter()
            .map(|column| Row::get::<String, _>(column, "name"))
            .collect::<Vec<_>>();

        if column_names.iter().any(|name| name == "snapshot") {
            return Ok(());
        }

        warn!(
            "Detected legacy workspace_restore_state schema without snapshot column; columns={:?}; starting migration",
            column_names
        );

        let legacy_rows = sqlx::query(
            "SELECT session_id, agent_id, console_visible, active_tab, selected_run_id, selected_artifact_id, selected_permission_request_id, selected_diagnostic_id, restorable_run_state, updated_at FROM workspace_restore_state",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to load legacy workspace_restore_state rows")?;

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to start workspace_restore_state migration")?;

        sqlx::query("ALTER TABLE workspace_restore_state RENAME TO workspace_restore_state_legacy")
            .execute(&mut *tx)
            .await
            .context("Failed to rename legacy workspace_restore_state table")?;

        sqlx::query(
            "CREATE TABLE workspace_restore_state (
                session_id TEXT PRIMARY KEY,
                snapshot TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create migrated workspace_restore_state table")?;

        let mut migrated_rows = 0usize;
        let mut skipped_rows = 0usize;

        for row in legacy_rows {
            let session_id = row
                .try_get::<Option<String>, _>("session_id")
                .context("Failed to read legacy workspace_restore_state.session_id")?
                .unwrap_or_default();

            if session_id.is_empty() {
                skipped_rows += 1;
                warn!(
                    "Skipping legacy workspace_restore_state row because session_id is missing during snapshot migration"
                );
                continue;
            }

            let agent_id = row
                .try_get::<Option<String>, _>("agent_id")
                .context("Failed to read legacy workspace_restore_state.agent_id")?;
            let console_visible = row
                .try_get::<Option<i64>, _>("console_visible")
                .context("Failed to read legacy workspace_restore_state.console_visible")?
                .unwrap_or(0)
                != 0;
            let active_tab = row
                .try_get::<Option<String>, _>("active_tab")
                .context("Failed to read legacy workspace_restore_state.active_tab")?;
            let selected_run_id = row
                .try_get::<Option<String>, _>("selected_run_id")
                .context("Failed to read legacy workspace_restore_state.selected_run_id")?;
            let selected_artifact_id = row
                .try_get::<Option<String>, _>("selected_artifact_id")
                .context("Failed to read legacy workspace_restore_state.selected_artifact_id")?;
            let selected_permission_request_id = row
                .try_get::<Option<String>, _>("selected_permission_request_id")
                .context("Failed to read legacy workspace_restore_state.selected_permission_request_id")?;
            let selected_diagnostic_id = row
                .try_get::<Option<String>, _>("selected_diagnostic_id")
                .context("Failed to read legacy workspace_restore_state.selected_diagnostic_id")?;
            let restorable_run_state = row
                .try_get::<Option<String>, _>("restorable_run_state")
                .context("Failed to read legacy workspace_restore_state.restorable_run_state")?;
            let updated_at = row
                .try_get::<Option<i64>, _>("updated_at")
                .context("Failed to read legacy workspace_restore_state.updated_at")?
                .unwrap_or(0);

            let snapshot = json!({
                "session_id": session_id,
                "agent_id": agent_id,
                "console_visible": console_visible,
                "active_tab": active_tab,
                "selected_run_id": selected_run_id,
                "selected_artifact_id": selected_artifact_id,
                "selected_permission_request_id": selected_permission_request_id,
                "selected_diagnostic_id": selected_diagnostic_id,
                "restorable_run_state": restorable_run_state,
            });
            let snapshot_json =
                serde_json::to_string(&snapshot).context("Failed to serialize migrated workspace restore snapshot")?;

            sqlx::query("INSERT INTO workspace_restore_state (session_id, snapshot, updated_at) VALUES (?, ?, ?)")
                .bind(&session_id)
                .bind(snapshot_json)
                .bind(updated_at)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("Failed to insert migrated workspace_restore_state for session {session_id}")
                })?;

            migrated_rows += 1;
        }

        sqlx::query("DROP TABLE workspace_restore_state_legacy")
            .execute(&mut *tx)
            .await
            .context("Failed to drop legacy workspace_restore_state table")?;

        tx.commit()
            .await
            .context("Failed to commit workspace_restore_state migration")?;

        warn!(
            "Completed workspace_restore_state snapshot migration; migrated_rows={}, skipped_rows={}",
            migrated_rows, skipped_rows
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteManager;
    use anyhow::Result;
    use serde_json::Value;
    use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
    use tempfile::tempdir;

    #[tokio::test]
    async fn migrates_legacy_workspace_restore_state_schema() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("sessions.db");
        let pool =
            SqlitePool::connect_with(SqliteConnectOptions::new().filename(&db_path).create_if_missing(true)).await?;

        sqlx::query(
            "CREATE TABLE workspace_restore_state (
                user_id TEXT PRIMARY KEY,
                session_id TEXT,
                agent_id TEXT,
                console_visible INTEGER,
                active_tab TEXT,
                selected_run_id TEXT,
                selected_artifact_id TEXT,
                selected_permission_request_id TEXT,
                selected_diagnostic_id TEXT,
                restorable_run_state TEXT,
                updated_at INTEGER
            );",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO workspace_restore_state (
                user_id, session_id, agent_id, console_visible, active_tab,
                selected_run_id, selected_artifact_id, selected_permission_request_id,
                selected_diagnostic_id, restorable_run_state, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("user-1")
        .bind("session-1")
        .bind("agent-1")
        .bind(1_i64)
        .bind("overview")
        .bind("run-1")
        .bind("artifact-1")
        .bind("request-1")
        .bind("diagnostic-1")
        .bind("reattachable")
        .bind(123_i64)
        .execute(&pool)
        .await?;

        pool.close().await;

        let manager = SqliteManager::new(dir.path()).await?;

        let columns = sqlx::query("PRAGMA table_info(workspace_restore_state)")
            .fetch_all(&manager.pool)
            .await?;
        let column_names = columns
            .iter()
            .map(|column| Row::get::<String, _>(column, "name"))
            .collect::<Vec<_>>();

        assert!(column_names.iter().any(|name| name == "snapshot"));
        assert!(!column_names.iter().any(|name| name == "user_id"));

        let row =
            sqlx::query("SELECT session_id, snapshot, updated_at FROM workspace_restore_state WHERE session_id = ?")
                .bind("session-1")
                .fetch_one(&manager.pool)
                .await?;

        let snapshot_json: String = row.get("snapshot");
        let snapshot: Value = serde_json::from_str(&snapshot_json)?;

        assert_eq!(row.get::<String, _>("session_id"), "session-1");
        assert_eq!(row.get::<i64, _>("updated_at"), 123);
        assert_eq!(snapshot.get("session_id").and_then(Value::as_str), Some("session-1"));
        assert_eq!(snapshot.get("agent_id").and_then(Value::as_str), Some("agent-1"));
        assert_eq!(snapshot.get("console_visible").and_then(Value::as_bool), Some(true));
        assert_eq!(snapshot.get("active_tab").and_then(Value::as_str), Some("overview"));
        assert_eq!(snapshot.get("selected_run_id").and_then(Value::as_str), Some("run-1"));
        assert_eq!(
            snapshot.get("selected_artifact_id").and_then(Value::as_str),
            Some("artifact-1")
        );
        assert_eq!(
            snapshot.get("selected_permission_request_id").and_then(Value::as_str),
            Some("request-1")
        );
        assert_eq!(
            snapshot.get("selected_diagnostic_id").and_then(Value::as_str),
            Some("diagnostic-1")
        );
        assert_eq!(
            snapshot.get("restorable_run_state").and_then(Value::as_str),
            Some("reattachable")
        );

        Ok(())
    }
}
