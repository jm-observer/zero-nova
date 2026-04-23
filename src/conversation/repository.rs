use crate::message::{ContentBlock, Role};
use anyhow::Result;
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
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, title, agent_id, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET 
                title=excluded.title, 
                agent_id=excluded.agent_id, 
                updated_at=excluded.updated_at",
        )
        .bind(id)
        .bind(title)
        .bind(agent_id)
        .bind(created_at)
        .bind(updated_at)
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
    ) -> Result<Option<(String, String, String, i64, i64, Vec<crate::message::Message>)>> {
        let row = sqlx::query("SELECT id, title, agent_id, created_at, updated_at FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let id: String = row.get("id");
            let title: String = row.get("title");
            let agent_id: String = row.get("agent_id");
            let created_at: i64 = row.get("created_at");
            let updated_at: i64 = row.get("updated_at");

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
                history.push(crate::message::Message { role, content });
            }

            return Ok(Some((id, title, agent_id, created_at, updated_at, history)));
        }

        Ok(None)
    }

    pub async fn list_sessions(&self) -> Result<Vec<(String, String, String, i64, i64)>> {
        let rows =
            sqlx::query("SELECT id, title, agent_id, created_at, updated_at FROM sessions ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push((
                row.get("id"),
                row.get("title"),
                row.get("agent_id"),
                row.get("created_at"),
                row.get("updated_at"),
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
}
