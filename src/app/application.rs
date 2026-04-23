use crate::app::conversation_service::ConversationService;
use crate::config::AppConfig;
use crate::message::{ContentBlock, Message, Role};
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Gateway 应用门面，整合核心业务服务、配置与持久化路径
pub struct GatewayApplication<C: LlmClient> {
    conversation_service: ConversationService<C>,
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
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

    pub async fn session_exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.conversation_service.sessions.get(session_id).await?.is_some())
    }

    pub async fn start_turn(
        &self,
        session_id: &str,
        input: &str,
        event_tx: tokio::sync::mpsc::Sender<crate::event::AgentEvent>,
    ) -> Result<()> {
        self.conversation_service.start_turn(session_id, input, event_tx).await
    }

    pub async fn stop_turn(&self, session_id: &str) -> Result<()> {
        self.conversation_service.stop_turn(session_id).await
    }

    pub async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<crate::gateway::protocol::Agent> {
        let agent = self.conversation_service.switch_agent(session_id, agent_id).await?;
        Ok(agent_to_protocol(&agent))
    }

    pub async fn list_sessions(&self) -> Vec<crate::gateway::protocol::Session> {
        self.conversation_service
            .sessions
            .list_sorted()
            .await
            .into_iter()
            .map(|summary| session_summary_to_protocol(&summary))
            .collect()
    }

    pub async fn session_messages(&self, session_id: &str) -> Result<Vec<crate::gateway::protocol::MessageDTO>> {
        let session = self
            .conversation_service
            .sessions
            .get(session_id)
            .await?
            .context("Session not found")?;
        Ok(messages_to_protocol(&session.get_internal_messages()))
    }

    pub async fn create_session(
        &self,
        title: Option<String>,
        agent_id: String,
    ) -> Result<crate::gateway::protocol::Session> {
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

        Ok(session_to_protocol(&session))
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        self.conversation_service.sessions.delete(session_id).await
    }

    pub async fn copy_session(
        &self,
        session_id: &str,
        truncate_index: Option<usize>,
    ) -> Result<crate::gateway::protocol::Session> {
        let session = self
            .conversation_service
            .sessions
            .copy_session(session_id, truncate_index)
            .await?
            .context("Source session not found")?;
        Ok(session_to_protocol(&session))
    }

    pub fn list_agents(&self) -> Vec<crate::gateway::protocol::Agent> {
        self.conversation_service
            .agent_registry
            .list()
            .into_iter()
            .map(agent_to_protocol)
            .collect()
    }

    pub fn get_agent(&self, agent_id: &str) -> Option<crate::gateway::protocol::Agent> {
        self.conversation_service
            .agent_registry
            .get(agent_id)
            .map(agent_to_protocol)
    }

    pub fn config_snapshot(&self) -> Result<Value> {
        let config = self.config.read().unwrap();
        serde_json::to_value(&*config).context("Failed to serialize config")
    }

    pub async fn update_config(&self, payload: Value) -> Result<()> {
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
    pub async fn connect(&self) -> Result<Vec<crate::gateway::protocol::GatewayMessage>> {
        Ok(vec![crate::gateway::protocol::GatewayMessage::new_event(
            crate::gateway::protocol::MessageEnvelope::Welcome(crate::gateway::protocol::WelcomePayload {
                require_auth: false,
                setup_required: false,
            }),
        )])
    }

    #[cfg(feature = "gateway")]
    pub async fn handle(
        &self,
        msg: crate::gateway::protocol::GatewayMessage,
        outbound_tx: channel_websocket::ResponseSink<crate::gateway::protocol::GatewayMessage>,
    ) {
        crate::gateway::router::handle_message(msg, self, outbound_tx).await;
    }

    #[cfg(feature = "gateway")]
    pub async fn disconnect(&self, peer: std::net::SocketAddr) {
        log::info!("Gateway peer disconnected: {}", peer);
    }
}

fn session_to_protocol(session: &Arc<crate::conversation::session::Session>) -> crate::gateway::protocol::Session {
    crate::gateway::protocol::Session {
        id: session.id.clone(),
        title: Some(session.name.clone()),
        agent_id: session.control.read().unwrap().active_agent.clone(),
        created_at: session.created_at,
        updated_at: session.updated_at.load(std::sync::atomic::Ordering::SeqCst),
        message_count: session.history.read().unwrap().len(),
    }
}

fn session_summary_to_protocol(
    summary: &crate::conversation::session::SessionSummary,
) -> crate::gateway::protocol::Session {
    crate::gateway::protocol::Session {
        id: summary.id.clone(),
        title: Some(summary.name.clone()),
        agent_id: summary.agent_id.clone(),
        created_at: summary.created_at,
        updated_at: summary.updated_at,
        message_count: summary.message_count,
    }
}

fn messages_to_protocol(messages: &[Message]) -> Vec<crate::gateway::protocol::MessageDTO> {
    messages
        .iter()
        .map(|message| crate::gateway::protocol::MessageDTO {
            role: match message.role {
                Role::System => "system".to_string(),
                Role::User => "user".to_string(),
                Role::Assistant => "assistant".to_string(),
            },
            content: message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => {
                        crate::gateway::protocol::ContentBlockDTO::Text { text: text.clone() }
                    }
                    ContentBlock::Thinking { thinking } => crate::gateway::protocol::ContentBlockDTO::Thinking {
                        thinking: thinking.clone(),
                    },
                    ContentBlock::ToolUse { id, name, input } => crate::gateway::protocol::ContentBlockDTO::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                    } => crate::gateway::protocol::ContentBlockDTO::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: output.clone(),
                        is_error: *is_error,
                    },
                })
                .collect(),
            timestamp: 0,
        })
        .collect()
}

fn agent_to_protocol(agent: &crate::agent_catalog::AgentDescriptor) -> crate::gateway::protocol::Agent {
    crate::gateway::protocol::Agent {
        id: agent.id.clone(),
        name: agent.display_name.clone(),
        description: Some(agent.description.clone()),
        icon: None,
        system_prompt: None,
    }
}
