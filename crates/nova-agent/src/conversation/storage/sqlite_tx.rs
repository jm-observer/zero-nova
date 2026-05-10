use anyhow::Result;
use sqlx::Acquire;

pub(crate) async fn delete_session_with_messages(pool: &sqlx::SqlitePool, session_id: &str) -> Result<()> {
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;
    sqlx::query("DELETE FROM messages WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
