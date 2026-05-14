use super::{parse_session_row, SessionRow, SqliteSessionRepository};
use crate::message::{ContentBlock, Message, Role};
use anyhow::Result;
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
}
