use crate::config::AppConfig;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait ConfigSnapshot: Send + Sync {
    /// Return a reference-counted copy of the current config.
    /// Using Arc<AppConfig> enables lock-free reads without cloning.
    async fn current(&self) -> Arc<AppConfig>;

    async fn apply(&self, _next: AppConfig) -> Result<()> {
        anyhow::bail!("Config apply is not supported")
    }
}
