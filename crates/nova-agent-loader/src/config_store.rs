use anyhow::Result;
use async_trait::async_trait;
use nova_agent::app::ConfigSnapshot;
use nova_agent_config::AppConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait ConfigListener: Send + Sync {
    async fn on_config_changed(&self, config: AppConfig) -> Result<()>;
}

#[derive(Clone)]
pub struct ConfigStore {
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
    config_dir: PathBuf,
    listeners: Arc<RwLock<Vec<Arc<dyn ConfigListener>>>>,
}

impl ConfigStore {
    pub fn new(initial: AppConfig) -> Self {
        let config_path = initial.config_path();
        let config_dir = initial.config_dir.clone();
        Self {
            config: Arc::new(RwLock::new(initial)),
            config_path,
            config_dir,
            listeners: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn handle(&self) -> Arc<RwLock<AppConfig>> {
        self.config.clone()
    }

    pub async fn current(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    pub async fn add_listener(&self, listener: Arc<dyn ConfigListener>) {
        self.listeners.write().await.push(listener);
    }

    pub async fn reload_from_disk(&self) -> Result<AppConfig> {
        let next = AppConfig::load_from_file(&self.config_path, self.config_dir.clone())?;
        self.store_and_notify(next.clone()).await?;
        Ok(next)
    }

    pub async fn apply(&self, next: AppConfig) -> Result<AppConfig> {
        self.store_and_notify(next.clone()).await?;
        Ok(next)
    }

    async fn store_and_notify(&self, next: AppConfig) -> Result<()> {
        {
            let mut guard = self.config.write().await;
            *guard = next.clone();
        }
        let listeners = self.listeners.read().await.clone();
        for listener in listeners {
            listener.on_config_changed(next.clone()).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl ConfigSnapshot for ConfigStore {
    async fn current(&self) -> AppConfig {
        ConfigStore::current(self).await
    }

    async fn apply(&self, next: AppConfig) -> Result<()> {
        let _ = ConfigStore::apply(self, next).await?;
        Ok(())
    }
}
