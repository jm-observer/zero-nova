use anyhow::{Context, Result};
use nova_conversation::SessionService;
use nova_core::agent::AgentRuntime;
use nova_core::agent_catalog::AgentRegistry;
use nova_core::event::AgentEvent;
use nova_core::provider::LlmClient;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// 核心会话业务服务
pub struct ConversationService<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
}

impl<C: LlmClient + 'static> ConversationService<C> {
    pub fn new(agent: AgentRuntime<C>, agent_registry: AgentRegistry, sessions: SessionService) -> Self {
        Self {
            agent,
            agent_registry,
            sessions,
        }
    }

    /// 执行一轮对话逻辑
    pub async fn start_turn(&self, session_id: &str, input: &str, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
        self.execute_agent_turn(session_id, input, event_tx).await
    }

    pub async fn stop_turn(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        if let Some(token) = session.take_cancellation_token() {
            token.cancel();
        }
        Ok(())
    }

    pub async fn switch_agent(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<nova_core::agent_catalog::AgentDescriptor> {
        let agent = self
            .agent_registry
            .get(agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;

        self.sessions.set_active_agent(session_id, agent_id).await?;

        Ok(agent)
    }

    async fn execute_agent_turn(
        &self,
        session_id: &str,
        input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let _lock = session.chat_lock.lock().await;

        self.sessions
            .append_message(
                session_id,
                nova_core::message::Role::User,
                vec![nova_core::message::ContentBlock::Text {
                    text: input.to_string(),
                }],
            )
            .await?;

        let token = CancellationToken::new();
        session.set_cancellation_token(token.clone());

        let history = session.get_history();
        let history_for_turn = &history[..history.len() - 1];

        let turn_result = self
            .agent
            .run_turn(history_for_turn, input, event_tx, Some(token))
            .await?;

        for msg in turn_result.messages {
            self.sessions.append_message(session_id, msg.role, msg.content).await?;
        }

        session.clear_cancellation_token();
        session.touch_updated_at();
        Ok(())
    }
}
