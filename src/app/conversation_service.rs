use crate::agent::AgentRuntime;
use crate::agent_catalog::AgentRegistry;
use crate::conversation::control::{InteractionKind, InteractionResolver, ResolutionIntent, TurnIntent, TurnRouter};
use crate::conversation::workflow::WorkflowEngine;
use crate::conversation::SessionStore;
use crate::event::AgentEvent;
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// 核心会话业务服务
pub struct ConversationService<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub agent_registry: AgentRegistry,
    pub sessions: SessionStore,
}

impl<C: LlmClient + 'static> ConversationService<C> {
    pub fn new(agent: AgentRuntime<C>, agent_registry: AgentRegistry, sessions: SessionStore) -> Self {
        Self {
            agent,
            agent_registry,
            sessions,
        }
    }

    /// 执行一轮对话逻辑
    pub async fn start_turn(&self, session_id: &str, input: &str, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;

        let _lock = session.chat_lock.lock().await;

        let intent = {
            let control = session.control.read().unwrap();
            TurnRouter::classify(input, &control, Some(&self.agent_registry))
        };

        match intent {
            TurnIntent::ResolvePendingInteraction => self.resolve_interaction(session_id, input, event_tx).await,
            TurnIntent::AddressAgent { agent_id } => self.request_agent_switch(session_id, &agent_id, event_tx).await,
            TurnIntent::ContinueWorkflow => self.advance_workflow(session_id, input, event_tx).await,
            TurnIntent::StartNewTask { topic } => self.start_workflow(session_id, &topic, input, event_tx).await,
            TurnIntent::ExecuteChat => self.execute_agent_turn(session_id, input, event_tx).await,
        }
    }

    pub async fn stop_turn(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        if let Some(token) = session.take_cancellation_token() {
            token.cancel();
        }
        Ok(())
    }

    async fn execute_agent_turn(
        &self,
        session_id: &str,
        input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;

        self.sessions
            .append_message(
                session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
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

    async fn resolve_interaction(
        &self,
        session_id: &str,
        input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;

        let resolution_data = {
            let mut control = session.control.write().unwrap();
            let pending = control.pending_interaction.take();
            if let Some(p) = pending {
                let res = InteractionResolver::resolve(input, &p);
                Some((p, res))
            } else {
                None
            }
        };

        if let Some((pending, resolution)) = resolution_data {
            if resolution.intent == ResolutionIntent::Approve && pending.id.starts_with("switch:") {
                let target_id = &pending.id[7..];
                {
                    let mut control = session.control.write().unwrap();
                    control.active_agent = target_id.to_string();
                }

                if let Some(agent) = self.agent_registry.get(target_id) {
                    let _ = event_tx
                        .send(AgentEvent::AgentSwitched {
                            agent_id: agent.id.clone(),
                            agent_name: agent.display_name.clone(),
                            description: Some(agent.description.clone()),
                        })
                        .await;
                }
            }

            let _ = event_tx
                .send(AgentEvent::InteractionResolved {
                    interaction_id: pending.id.clone(),
                    result: map_resolution_result(resolution.intent).to_string(),
                })
                .await;
        }
        Ok(())
    }

    async fn request_agent_switch(
        &self,
        session_id: &str,
        agent_id: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        if let Some(agent) = self.agent_registry.get(agent_id) {
            let interaction_id = format!("switch:{}", agent_id);
            let pending = crate::conversation::control::PendingInteraction {
                id: interaction_id.clone(),
                kind: InteractionKind::Approve,
                subject: "Agent Switch".to_string(),
                prompt: format!("您是否要切换到 {}？", agent.display_name),
                options: vec![],
                risk_level: crate::conversation::control::RiskLevel::Low,
                created_at: chrono::Utc::now().timestamp_millis(),
                ttl_seconds: 300,
            };

            {
                let mut control = session.control.write().unwrap();
                control.pending_interaction = Some(pending.clone());
            }

            let _ = event_tx
                .send(AgentEvent::InteractionRequest {
                    interaction_id,
                    kind: "approve".to_string(),
                    subject: "Agent Switch".to_string(),
                    prompt: format!("您是否要切换到 {}？", agent.display_name),
                    options: vec![],
                })
                .await;
        }
        Ok(())
    }

    async fn advance_workflow(&self, session_id: &str, input: &str, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let mut wf = {
            let control = session.control.read().unwrap();
            control.workflow.clone().context("No active workflow")?
        };

        let result = WorkflowEngine::advance(&mut wf, input, &self.agent, event_tx.clone()).await?;

        {
            let mut control = session.control.write().unwrap();
            control.workflow = Some(wf);
            if let Some(pending) = result.new_pending {
                control.pending_interaction = Some(pending);
            }
        }

        self.sessions
            .append_message(
                session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: input.to_string(),
                }],
            )
            .await?;

        for msg in result.messages {
            self.sessions
                .append_message(
                    session_id,
                    crate::message::Role::Assistant,
                    vec![crate::message::ContentBlock::Text { text: msg }],
                )
                .await?;
        }

        Ok(())
    }

    async fn start_workflow(
        &self,
        session_id: &str,
        topic: &str,
        input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let mut wf = crate::conversation::workflow::WorkflowState::new(topic.to_string());

        let result = WorkflowEngine::advance(&mut wf, input, &self.agent, event_tx).await?;

        {
            let mut control = session.control.write().unwrap();
            control.workflow = Some(wf);
            if let Some(pending) = result.new_pending {
                control.pending_interaction = Some(pending);
            }
        }

        self.sessions
            .append_message(
                session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: input.to_string(),
                }],
            )
            .await?;

        for msg in result.messages {
            self.sessions
                .append_message(
                    session_id,
                    crate::message::Role::Assistant,
                    vec![crate::message::ContentBlock::Text { text: msg }],
                )
                .await?;
        }

        Ok(())
    }
}

fn map_resolution_result(intent: ResolutionIntent) -> &'static str {
    match intent {
        ResolutionIntent::Approve => "approved",
        ResolutionIntent::Reject => "rejected",
        ResolutionIntent::Select => "selected",
        ResolutionIntent::ProvideInput => "input",
        ResolutionIntent::Unclear => "expired",
    }
}
