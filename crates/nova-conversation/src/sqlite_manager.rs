use anyhow::{Context, Result};
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::path::Path;
use tokio::fs;

pub struct SqliteManager {
    pub pool: SqlitePool,
    pub db_path: String,
}

impl SqliteManager {
    pub async fn new(data_dir: &str) -> Result<Self> {
        let data_path = Path::new(data_dir);
        if !data_path.exists() {
            fs::create_dir_all(data_path)
                .await
                .context("Failed to create data directory")?;
        }

        let db_path = data_path.join("sessions.db").to_str().unwrap().to_string();

        let options = SqliteConnectOptions::new().filename(&db_path).create_if_missing(true);

        let pool = SqlitePool::connect_with(options)
            .await
            .context("Failed to connect to SQLite")?;

        let manager = Self { pool, db_path };

        manager.run_migrations().await?;

        Ok(manager)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                agent_id TEXT,
                created_at INTEGER,
                updated_at INTEGER
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create sessions table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create messages table")?;

        self.migrate_messages_timestamp_column().await?;

        Ok(())
    }

    async fn migrate_messages_timestamp_column(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(messages)")
            .fetch_all(&self.pool)
            .await
            .context("Failed to inspect messages table schema")?;

        let mut has_created_at = false;
        let mut has_timestamp = false;

        for column in columns {
            let name: String = sqlx::Row::get(&column, "name");
            if name == "created_at" {
                has_created_at = true;
            } else if name == "timestamp" {
                has_timestamp = true;
            }
        }

        if !has_created_at && has_timestamp {
            sqlx::query("ALTER TABLE messages ADD COLUMN created_at INTEGER")
                .execute(&self.pool)
                .await
                .context("Failed to add created_at column to messages table")?;

            sqlx::query("UPDATE messages SET created_at = timestamp WHERE created_at IS NULL")
                .execute(&self.pool)
                .await
                .context("Failed to backfill created_at from timestamp")?;
        }

        Ok(())
    }
}
