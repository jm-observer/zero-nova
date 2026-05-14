use super::SqliteSessionRepository;
use anyhow::Result;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn create_artifact(&self, artifact: &crate::conversation::model::ArtifactRecord) -> Result<()> {
        sqlx::query("INSERT INTO artifacts (id, session_id, run_id, name, content_type, storage_path, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(&artifact.id)
            .bind(&artifact.session_id)
            .bind(&artifact.run_id)
            .bind(&artifact.name)
            .bind(&artifact.content_type)
            .bind(&artifact.storage_path)
            .bind(artifact.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_artifacts(&self, session_id: &str) -> Result<Vec<crate::conversation::model::ArtifactRecord>> {
        let rows = sqlx::query("SELECT id, session_id, run_id, name, content_type, storage_path, created_at FROM artifacts WHERE session_id = ? ORDER BY created_at DESC")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let mut artifacts = Vec::new();
        for row in rows {
            artifacts.push(crate::conversation::model::ArtifactRecord {
                id: row.get("id"),
                session_id: row.get("session_id"),
                run_id: row.get("run_id"),
                name: row.get("name"),
                content_type: row.get("content_type"),
                storage_path: row.get("storage_path"),
                created_at: row.get("created_at"),
            });
        }
        Ok(artifacts)
    }
}
