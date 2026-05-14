use super::SqliteSessionRepository;
use anyhow::{Context, Result};
use log::warn;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn create_audit_log(&self, log: &crate::conversation::model::AuditLogRecord) -> Result<()> {
        let details_json = serde_json::to_string(&log.details)
            .with_context(|| format!("Failed to serialize audit_log.details for '{}'", log.action))?;
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

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<Vec<crate::conversation::model::AuditLogRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, action, details, created_at FROM audit_logs WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut logs = Vec::new();
        for row in rows {
            let details_json: String = row.get("details");
            let details: serde_json::Value = serde_json::from_str(&details_json).unwrap_or_else(|e| {
                warn!(
                    "Failed to decode audit log details for '{:?}' (session '{}', action '{}'): {e} — using null",
                    row.get::<String, _>("id"),
                    row.get::<String, _>("session_id"),
                    row.get::<String, _>("action")
                );
                serde_json::Value::Null
            });
            logs.push(crate::conversation::model::AuditLogRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                action: row.get("action"),
                details,
                created_at: row.get("created_at"),
            });
        }
        Ok(logs)
    }
}
