use super::SqliteSessionRepository;
use anyhow::{Context, Result};
use sqlx::Row;

impl SqliteSessionRepository {
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
}
