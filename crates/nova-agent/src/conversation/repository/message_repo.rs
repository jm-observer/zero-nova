use super::SqliteSessionRepository;
use crate::message::{ContentBlock, Role};
use anyhow::Result;
use serde_json::Value;

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
}
