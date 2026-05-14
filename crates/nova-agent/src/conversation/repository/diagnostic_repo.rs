use super::SqliteSessionRepository;
use anyhow::{Context, Result};
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn create_diagnostic_issue(&self, issue: &crate::conversation::model::DiagnosticIssue) -> Result<()> {
        let details_json = issue
            .details
            .as_ref()
            .map(|v| {
                serde_json::to_string(v)
                    .with_context(|| format!("Failed to serialize diagnostic.details for '{}'", issue.id))
            })
            .transpose()?;
        sqlx::query("INSERT INTO diagnostic_issues (id, session_id, severity, message, details, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&issue.id)
            .bind(&issue.session_id)
            .bind(&issue.severity)
            .bind(&issue.message)
            .bind(details_json)
            .bind(issue.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_diagnostics(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM diagnostic_issues WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_diagnostics(&self, session_id: &str) -> Result<Vec<crate::conversation::model::DiagnosticIssue>> {
        let rows = sqlx::query("SELECT id, session_id, severity, message, details, created_at FROM diagnostic_issues WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut issues = Vec::new();
        for row in rows {
            let details_json: Option<String> = row.get("details");
            issues.push(crate::conversation::model::DiagnosticIssue {
                id: row.get("id"),
                session_id: row.get("session_id"),
                severity: row.get("severity"),
                message: row.get("message"),
                details: details_json.and_then(|j| serde_json::from_str(&j).ok()),
                created_at: row.get("created_at"),
            });
        }
        Ok(issues)
    }
}
