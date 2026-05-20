use super::{parse_session_row, SessionRow, SqliteSessionRepository};
use crate::message::{ContentBlock, Message, Role};
use anyhow::Result;
use log::warn;
use sqlx::{Acquire, Row};

impl SqliteSessionRepository {
    // 9 参数：与 sessions 表列一一对应；引入参数结构体会让 6 个既有调用点都被迫改造，
    // 收益与代价不匹配。保留扁平签名。
    #[allow(clippy::too_many_arguments)]
    pub async fn save_session(
        &self,
        id: &str,
        title: &str,
        agent_id: &str,
        created_at: i64,
        updated_at: i64,
        runtime_control: &crate::conversation::control::ControlState,
        parent_session_id: Option<&str>,
        parent_tool_use_id: Option<&str>,
    ) -> Result<()> {
        let runtime_control_json = serde_json::to_string(runtime_control)?;
        // ON CONFLICT 子句中 parent_session_id / parent_tool_use_id 故意不出现：
        // 子 Session 创建后这两列永不被 UPDATE 覆盖（一次写定语义）。
        sqlx::query(
            "INSERT INTO sessions \
             (id, title, agent_id, created_at, updated_at, runtime_control, \
              parent_session_id, parent_tool_use_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
                title = excluded.title, \
                agent_id = excluded.agent_id, \
                updated_at = excluded.updated_at, \
                runtime_control = excluded.runtime_control",
        )
        .bind(id)
        .bind(title)
        .bind(agent_id)
        .bind(created_at)
        .bind(updated_at)
        .bind(runtime_control_json)
        .bind(parent_session_id)
        .bind(parent_tool_use_id)
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
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control, \
                    parent_session_id, parent_tool_use_id \
             FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session_row).transpose()
    }

    #[allow(clippy::type_complexity)]
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
            Option<String>, // parent_session_id
            Option<String>, // parent_tool_use_id
        )>,
    > {
        let row = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control, \
                    parent_session_id, parent_tool_use_id \
             FROM sessions WHERE id = ?",
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
            let parent_session_id: Option<String> = row.try_get("parent_session_id").unwrap_or(None);
            let parent_tool_use_id: Option<String> = row.try_get("parent_tool_use_id").unwrap_or(None);

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
                parent_session_id,
                parent_tool_use_id,
            )));
        }

        Ok(None)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let rows = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control, \
                    parent_session_id, parent_tool_use_id \
             FROM sessions ORDER BY updated_at DESC",
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
            let parent_session_id: Option<String> = row.try_get("parent_session_id").unwrap_or(None);
            let parent_tool_use_id: Option<String> = row.try_get("parent_tool_use_id").unwrap_or(None);

            sessions.push((
                row.get("id"),
                row.get("title"),
                agent_id,
                row.get("created_at"),
                row.get("updated_at"),
                runtime_control,
                parent_session_id,
                parent_tool_use_id,
            ));
        }
        Ok(sessions)
    }

    pub async fn find_latest_session_by_agent(&self, agent_id: &str) -> Result<Option<SessionRow>> {
        let row = sqlx::query(
            "SELECT id, title, agent_id, created_at, updated_at, runtime_control, \
                    parent_session_id, parent_tool_use_id \
             FROM sessions \
             WHERE agent_id = ? \
             ORDER BY updated_at DESC \
             LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session_row).transpose()
    }

    /// 返回指定 parent 的所有直接子 Session id，按 created_at 升序。
    /// 无子或 parent 不存在时返回空 vec（不报错）。
    pub async fn list_child_session_ids(&self, parent_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT id FROM sessions WHERE parent_session_id = ? ORDER BY created_at, id")
            .bind(parent_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
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
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;
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

    /// 单 transaction 批量删 session 行 + 其 messages。空列表直接返回 Ok。
    /// 不存在的 id 静默跳过（DELETE 是幂等的）。
    pub async fn delete_sessions_bulk(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;
        for id in ids {
            sqlx::query("DELETE FROM messages WHERE session_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
