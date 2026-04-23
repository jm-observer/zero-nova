use crate::app::conversation_service::ConversationService;
use crate::config::AppConfig;
use crate::provider::LlmClient;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Gateway 应用门面，整合核心业务服务、配置与持久化路径
pub struct GatewayApplication<C: LlmClient> {
    pub conversation_service: ConversationService<C>,
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: PathBuf,
}

impl<C: LlmClient + 'static> GatewayApplication<C> {
    pub fn new(
        conversation_service: ConversationService<C>,
        config: Arc<RwLock<AppConfig>>,
        config_path: PathBuf,
    ) -> Self {
        Self {
            conversation_service,
            config,
            config_path,
        }
    }
}
