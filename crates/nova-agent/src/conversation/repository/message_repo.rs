use super::SqliteSessionRepository;
use crate::message::{ContentBlock, Role};
use anyhow::Result;
use serde_json::Value;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn save_message(
        &self,
        session_id: &str,
        message_id: &str,
        role: Role,
        content: Vec<ContentBlock>,
        metadata: Option<Value>,
        created_at: i64,
    ) -> Result<()> {
        let role_str = match role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let content_json = serde_json::to_string(&content)?;
        let metadata_json = metadata.map(|v| serde_json::to_string(&v)).transpose()?;

        sqlx::query(
            "INSERT INTO messages (session_id, message_id, role, content, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(message_id)
        .bind(role_str)
        .bind(content_json)
        .bind(metadata_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 返回指定 session 的消息总数。用于 SessionSummary 的 message_count 字段，避免 load 完整 history。
    pub async fn count_messages(&self, session_id: &str) -> Result<usize> {
        let row = sqlx::query("SELECT COUNT(*) AS c FROM messages WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await?;
        let count: i64 = row.get::<i64, _>("c");
        Ok(count.max(0) as usize)
    }
}
