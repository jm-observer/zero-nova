use crate::config::AppConfig;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ConfigSnapshot: Send + Sync {
    async fn current(&self) -> AppConfig;

    async fn apply(&self, _next: AppConfig) -> Result<()> {
        anyhow::bail!("Config apply is not supported")
    }
}
