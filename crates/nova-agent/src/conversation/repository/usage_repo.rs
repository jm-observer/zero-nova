use super::{SessionUsageAggregate, SqliteSessionRepository, UsageQualityCounts};
use anyhow::Result;
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn sum_session_usage(&self, session_id: &str) -> Result<SessionUsageAggregate> {
        let row = sqlx::query(
            "SELECT
                COALESCE(SUM(json_extract(usage, '$.inputTokens')), 0) AS input_tokens,
                COALESCE(SUM(json_extract(usage, '$.outputTokens')), 0) AS output_tokens,
                COALESCE(SUM(json_extract(usage, '$.cacheCreationInputTokens')), 0) AS cache_creation_input_tokens,
                COALESCE(SUM(json_extract(usage, '$.cacheReadInputTokens')), 0) AS cache_read_input_tokens
             FROM runs WHERE session_id = ? AND usage IS NOT NULL",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(SessionUsageAggregate {
            input_tokens: row.get::<i64, _>("input_tokens") as u64,
            output_tokens: row.get::<i64, _>("output_tokens") as u64,
            cache_creation_input_tokens: row.get::<i64, _>("cache_creation_input_tokens") as u64,
            cache_read_input_tokens: row.get::<i64, _>("cache_read_input_tokens") as u64,
        })
    }

    pub async fn count_usage_quality(&self, session_id: &str) -> Result<UsageQualityCounts> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) AS total_turns,
                COUNT(CASE WHEN usage IS NOT NULL
                    AND json_extract(usage, '$.cacheCreationInputTokens') IS NULL
                    AND json_extract(usage, '$.cacheReadInputTokens') IS NULL
                    THEN 1 END) AS turns_with_unknown_cache,
                COUNT(CASE WHEN usage IS NULL THEN 1 END) AS turns_with_missing_usage
             FROM runs WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(UsageQualityCounts {
            total_turns: row.get::<i64, _>("total_turns") as u32,
            turns_with_unknown_cache: row.get::<i64, _>("turns_with_unknown_cache") as u32,
            turns_with_missing_usage: row.get::<i64, _>("turns_with_missing_usage") as u32,
        })
    }
}
