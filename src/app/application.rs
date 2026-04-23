use crate::app::conversation_service::ConversationService;
use crate::app::types::{AppAgent, AppEvent, AppMessage, AppSession};
use crate::config::AppConfig;
use crate::message::Role;
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::mpsc;

#[async_trait]
pub trait GatewayApplication: Send + Sync {
    async fn session_exists(&self, session_id: &str) -> Result<bool>;
    async fn start_turn(&self, session_id: &str, input: &str, sender: mpsc::Sender<AppEvent>) -> Result<()>;
    async fn stop_turn(&self, session_id: &str) -> Result<()>;

    async fn list_sessions(&self) -> Result<Vec<AppSession>>;
    async fn session_messages(&self, session_id: &str) -> Result<Vec<AppMessage>>;
    async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession>;
    async fn delete_session(&self, session_id: &str) -> Result<bool>;
    async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>) -> Result<AppSession>;

    async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgent>;
    fn list_agents(&self) -> Vec<AppAgent>;
    fn get_agent(&self, agent_id: &str) -> Option<AppAgent>;

    fn config_snapshot(&self) -> Result<Value>;
    async fn update_config(&self, payload: Value) -> Result<()>;

    #[cfg(feature = "gateway")]
    async fn connect(&self) -> Result<Vec<crate::gateway::protocol::GatewayMessage>>;
    #[cfg(feature = "gateway")]
    async fn handle(
        &self,
        msg: crate::gateway::protocol::GatewayMessage,
        outbound_tx: channel_websocket::ResponseSink<crate::gateway::protocol::GatewayMessage>,
    );
    #[cfg(feature = "gateway")]
    async fn disconnect(&self, peer: std::net::SocketAddr);
}

/// Gateway 应用门面实现
pub struct GatewayApplicationImpl<C: LlmClient> {
    conversation_service: ConversationService<C>,
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
}

impl<C: LlmClient + 'static> GatewayApplicationImpl<C> {
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

#[async_trait]
impl<C: LlmClient + 'static> GatewayApplication for GatewayApplicationImpl<C> {
    async fn session_exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.conversation_service.sessions.get(session_id).await?.is_some())
    }

    async fn start_turn(&self, session_id: &str, input: &str, sender: mpsc::Sender<AppEvent>) -> Result<()> {
        let (agent_event_tx, mut agent_event_rx) = mpsc::channel(100);

        tokio::spawn(async move {
            while let Some(event) = agent_event_rx.recv().await {
                if sender.send(AppEvent::from(event)).await.is_err() {
                    break;
                }
            }
        });

        self.conversation_service
            .start_turn(session_id, input, agent_event_tx)
            .await
    }

    async fn stop_turn(&self, session_id: &str) -> Result<()> {
        self.conversation_service.stop_turn(session_id).await
    }

    async fn list_sessions(&self) -> Result<Vec<AppSession>> {
        let summaries = self.conversation_service.sessions.list_sorted().await;
        Ok(summaries
            .into_iter()
            .map(|s| AppSession {
                id: s.id,
                title: Some(s.name),
                agent_id: s.agent_id,
                created_at: s.created_at,
                updated_at: s.updated_at,
                message_count: s.message_count,
            })
            .collect())
    }

    async fn session_messages(&self, session_id: &str) -> Result<Vec<AppMessage>> {
        let session = self
            .conversation_service
            .sessions
            .get(session_id)
            .await?
            .context("Session not found")?;

        let messages = session.get_internal_messages();
        Ok(messages
            .into_iter()
            .map(|m| AppMessage {
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m.content,
                timestamp: 0,
            })
            .collect())
    }

    async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession> {
        let system_prompt = self
            .conversation_service
            .agent_registry
            .get(&agent_id)
            .map(|agent| agent.system_prompt_template.clone())
            .unwrap_or_default();

        let session = self
            .conversation_service
            .sessions
            .create(title, agent_id, system_prompt)
            .await?;

        let id = session.id.clone();
        let name = session.name.clone();
        let active_agent = session.control.read().unwrap().active_agent.clone();
        let created_at = session.created_at;
        let updated_at = session.updated_at.load(std::sync::atomic::Ordering::SeqCst);
        let message_count = session.history.read().unwrap().len();

        Ok(AppSession {
            id,
            title: Some(name),
            agent_id: active_agent,
            created_at,
            updated_at,
            message_count,
        })
    }

    async fn delete_session(&self, session_id: &str) -> Result<bool> {
        self.conversation_service.sessions.delete(session_id).await
    }

    async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>) -> Result<AppSession> {
        let session = self
            .conversation_service
            .sessions
            .copy_session(session_id, truncate_index)
            .await?
            .context("Source session not found")?;

        let id = session.id.clone();
        let name = session.name.clone();
        let active_agent = session.control.read().unwrap().active_agent.clone();
        let created_at = session.created_at;
        let updated_at = session.updated_at.load(std::sync::atomic::Ordering::SeqCst);
        let message_count = session.history.read().unwrap().len();

        Ok(AppSession {
            id,
            title: Some(name),
            agent_id: active_agent,
            created_at,
            updated_at,
            message_count,
        })
    }

    async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgent> {
        let agent = self.conversation_service.switch_agent(session_id, agent_id).await?;
        Ok(AppAgent {
            id: agent.id.clone(),
            name: agent.display_name.clone(),
            description: Some(agent.description.clone()),
        })
    }

    fn list_agents(&self) -> Vec<AppAgent> {
        self.conversation_service
            .agent_registry
            .list()
            .into_iter()
            .map(|agent| AppAgent {
                id: agent.id.clone(),
                name: agent.display_name.clone(),
                description: Some(agent.description.clone()),
            })
            .collect()
    }

    fn get_agent(&self, agent_id: &str) -> Option<AppAgent> {
        self.conversation_service
            .agent_registry
            .get(agent_id)
            .map(|agent| AppAgent {
                id: agent.id.clone(),
                name: agent.display_name.clone(),
                description: Some(agent.description.clone()),
            })
    }

    fn config_snapshot(&self) -> Result<Value> {
        let config = self.config.read().unwrap();
        serde_json::to_value(&*config).context("Failed to serialize config")
    }

    async fn update_config(&self, payload: Value) -> Result<()> {
        let new_config =
            serde_json::from_value::<AppConfig>(payload).context("Failed to parse config update payload")?;
        let config_str = toml::to_string(&new_config).context("Failed to serialize updated config")?;
        tokio::fs::write(&self.config_path, config_str)
            .await
            .with_context(|| format!("Failed to save config to {:?}", self.config_path))?;

        let mut config = self.config.write().unwrap();
        *config = new_config;
        Ok(())
    }

    #[cfg(feature = "gateway")]
    async fn connect(&self) -> Result<Vec<crate::gateway::protocol::GatewayMessage>> {
        Ok(vec![crate::gateway::protocol::GatewayMessage::new_event(
            crate::gateway::protocol::MessageEnvelope::Welcome(crate::gateway::protocol::WelcomePayload {
                require_auth: false,
                setup_required: false,
            }),
        )])
    }

    #[cfg(feature = "gateway")]
    async fn handle(
        &self,
        msg: crate::gateway::protocol::GatewayMessage,
        outbound_tx: channel_websocket::ResponseSink<crate::gateway::protocol::GatewayMessage>,
    ) {
        crate::gateway::router::handle_message(msg, self, outbound_tx).await;
    }

    #[cfg(feature = "gateway")]
    async fn disconnect(&self, peer: std::net::SocketAddr) {
        log::info!("Gateway peer disconnected: {}", peer);
    }
}
