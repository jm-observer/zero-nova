use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use nova_agent_config::AppConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait ConfigListener: Send + Sync {
    async fn on_config_changed(&self, config: Arc<AppConfig>) -> Result<()>;
}

/// Helper to extract AppConfig from ArcSwap's Guard<Arc<AppConfig>>
fn extract_appconfig(arc: &arc_swap::Guard<Arc<AppConfig>>) -> AppConfig {
    Arc::as_ref(&**arc).clone()
}

pub struct ConfigStore {
    config: ArcSwap<AppConfig>,
    config_path: PathBuf,
    config_dir: PathBuf,
    listeners: Arc<RwLock<Vec<Arc<dyn ConfigListener>>>>,
}

impl Clone for ConfigStore {
    fn clone(&self) -> Self {
        let arc = self.config.load();
        Self {
            config: ArcSwap::from_pointee(extract_appconfig(&arc)),
            config_path: self.config_path.clone(),
            config_dir: self.config_dir.clone(),
            listeners: self.listeners.clone(),
        }
    }
}

impl ConfigStore {
    pub fn new(initial: AppConfig) -> Self {
        let config_path = initial.config_path();
        let config_dir = initial.config_dir.clone();
        Self {
            config: ArcSwap::from_pointee(initial),
            config_path,
            config_dir,
            listeners: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn handle(&self) -> Arc<AppConfig> {
        let guard = self.config.load();
        Arc::clone(&*guard)
    }

    pub async fn current(&self) -> Arc<AppConfig> {
        self.handle()
    }

    pub async fn add_listener(&self, listener: Arc<dyn ConfigListener>) {
        self.listeners.write().await.push(listener);
    }

    pub async fn reload_from_disk(&self) -> Result<AppConfig> {
        let next = AppConfig::load_from_file(&self.config_path, self.config_dir.clone())?;
        self.store_and_notify(next.clone()).await?;
        let guard = self.config.load();
        Ok(extract_appconfig(&guard))
    }

    pub async fn apply(&self, next: AppConfig) -> Result<Arc<AppConfig>> {
        self.store_and_notify(next.clone()).await?;
        let guard = self.config.load();
        Ok(Arc::clone(&*guard))
    }

    async fn store_and_notify(&self, next: AppConfig) -> Result<()> {
        let arc = Arc::new(next);
        self.config.store(arc.clone());
        let listeners = self.listeners.read().await.clone();
        for listener in listeners {
            listener.on_config_changed(arc.clone()).await?;
        }
        Ok(())
    }
}
