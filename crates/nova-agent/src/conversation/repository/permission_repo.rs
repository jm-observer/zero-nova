use super::SqliteSessionRepository;
use anyhow::Result;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn create_permission_request(
        &self,
        req: &crate::conversation::model::PermissionRequestRecord,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO permission_requests (request_id, session_id, run_id, step_id, agent_id, kind, title, reason, target, risk_level, status, created_at, resolved_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&req.id)
        .bind(&req.session_id)
        .bind(&req.run_id)
        .bind("")
        .bind("")
        .bind(&req.capability)
        .bind(&req.resource)
        .bind(&req.reason)
        .bind(&req.resource)
        .bind("unknown")
        .bind(&req.status)
        .bind(req.created_at)
        .bind(Option::<i64>::None)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn resolve_permission_request(&self, id: &str, status: &str, _reason: Option<&str>) -> Result<()> {
        let resolved_at = if matches!(status, "allowed" | "denied") {
            Some(chrono::Utc::now().timestamp_millis())
        } else {
            None
        };

        sqlx::query("UPDATE permission_requests SET status = ?, resolved_at = ? WHERE request_id = ?")
            .bind(status)
            .bind(resolved_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_permission_requests(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::conversation::model::PermissionRequestRecord>> {
        let rows = sqlx::query(
            "SELECT request_id, session_id, run_id, kind, target, status, reason, created_at FROM permission_requests WHERE session_id = ? ORDER BY created_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut requests = Vec::new();
        for row in rows {
            requests.push(crate::conversation::model::PermissionRequestRecord {
                id: row.get("request_id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                capability: row.get("kind"),
                resource: row.get("target"),
                status: row.get("status"),
                reason: row.get("reason"),
                created_at: row.get("created_at"),
            });
        }
        Ok(requests)
    }
}
