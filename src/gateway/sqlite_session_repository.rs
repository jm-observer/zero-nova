use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};

pub struct SqliteSessionRepository {
    pool: SqlitePool,
}

impl SqliteSessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save_session(
        &self,
        id: &str,
        title: &str,
        agent_id: &str,
        created_at: i64,
        updated_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, title, agent_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                agent_id = excluded.agent_id,
                updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(title)
        .bind(agent_id)
        .bind(created_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .context("Failed to save session")?;

        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete session")?;
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
        let content_json = serde_json::to_string(&content).context("Failed to serialize message content")?;

        sqlx::query("INSERT INTO messages (session_id, role, content, created_at) VALUES (?, ?, ?, ?)")
            .bind(session_id)
            .bind(role_str)
            .bind(content_json)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .context("Failed to save message")?;

        Ok(())
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<(String, String, String, i64, i64, Vec<Message>)>> {
        let row = sqlx::query("SELECT id, title, agent_id, created_at, updated_at FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch session row")?;

        if let Some(s) = row {
            let s_id: String = s.get("id");
            let s_title: String = s.get::<Option<String>, _>("title").unwrap_or_default();
            let s_agent_id: String = s.get("agent_id");
            let s_created_at: i64 = s.get("created_at");
            let s_updated_at: i64 = s.get("updated_at");

            let mut messages = Vec::new();
            let msg_rows = sqlx::query(
                "SELECT role, content, created_at FROM messages WHERE session_id = ? ORDER BY created_at ASC",
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch messages")?;

            for m in msg_rows {
                let role_str: String = m.get("role");
                let role = match role_str.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    _ => Role::Assistant,
                };
                let content_str: String = m.get("content");
                let content: Vec<ContentBlock> =
                    serde_json::from_str(&content_str).context("Failed to deserialize message content")?;
                let _created_at: i64 = m.get("created_at");

                messages.push(Message { role, content });
            }

            Ok(Some((s_id, s_title, s_agent_id, s_created_at, s_updated_at, messages)))
        } else {
            Ok(None)
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<(String, String, String, i64, i64)>> {
        let rows =
            sqlx::query("SELECT id, title, agent_id, created_at, updated_at FROM sessions ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch sessions")?;

        let mut list = Vec::new();
        for r in rows {
            list.push((
                r.get("id"),
                r.get::<Option<String>, _>("title").unwrap_or_default(),
                r.get("agent_id"),
                r.get("created_at"),
                r.get("updated_at"),
            ));
        }
        Ok(list)
    }
}
