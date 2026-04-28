use crate::agent::AgentRuntime;
use crate::agent::TurnResult;
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::conversation::control::{LastTurnSnapshot, ModelRef};
use crate::conversation::model::{RunRecord, RunStepRecord};
use crate::conversation::SessionService;
use crate::event::AgentEvent;
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::{load_project_context_with_config_async, PromptConfig};
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;
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

    fn resolve_run_models(
        &self,
        session: &crate::conversation::session::Session,
        agent_descriptor: &AgentDescriptor,
    ) -> (Option<ModelRef>, Option<ModelRef>) {
        let control = session.control.read().unwrap();
        let default_model_name = agent_descriptor
            .model_config
            .as_ref()
            .map(|config| config.model.clone())
            .unwrap_or_else(|| self.agent.config.model_config.model.clone());
        let default_model = ModelRef {
            provider: "default".to_string(),
            model: default_model_name,
        };

        let orchestration_model = control
            .model_override
            .orchestration
            .clone()
            .or(Some(default_model.clone()));
        let execution_model = control.model_override.execution.clone().or(Some(default_model));

        (orchestration_model, execution_model)
    }

    /// 执行一轮对话逻辑
    pub async fn start_turn(
        &self,
        session_id: &str,
        input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<TurnResult> {
        self.execute_agent_turn(session_id, input, event_tx).await
    }

    pub async fn stop_turn(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        if let Some(token) = session.take_cancellation_token() {
            token.cancel();
        }
        Ok(())
    }

    pub async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AgentDescriptor> {
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
    ) -> Result<TurnResult> {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let run_id = turn_id.clone(); // Use turn_id as run_id for simplicity
        let now = Utc::now().timestamp_millis();

        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let agent_id = session.get_active_agent();
        let agent_descriptor = self
            .agent_registry
            .get(&agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;
        let (orchestration_model, execution_model) = self.resolve_run_models(&session, &agent_descriptor);

        // Phase 2: Create Run record
        self.sessions
            .get_repository()
            .create_run(&RunRecord {
                id: run_id.clone(),
                session_id: session_id.to_string(),
                status: "running".to_string(),
                created_at: now,
                updated_at: now,
                orchestration_model,
                execution_model,
                tool_call_count: Some(0),
            })
            .await?;

        let (recorded_tx, mut recorded_rx) = mpsc::channel(100);
        let repository = self.sessions.get_repository();
        let run_id_clone = run_id.clone();
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            while let Some(event) = recorded_rx.recv().await {
                match &event {
                    AgentEvent::ToolStart { id, name: _, input } => {
                        let _ = repository
                            .create_run_step(&RunStepRecord {
                                id: id.clone(),
                                run_id: run_id_clone.clone(),
                                step_type: "tool_use".to_string(),
                                status: "running".to_string(),
                                input: Some(input.clone()),
                                output: None,
                                created_at: Utc::now().timestamp_millis(),
                                updated_at: Utc::now().timestamp_millis(),
                            })
                            .await;
                    }
                    AgentEvent::ToolEnd {
                        id, output, is_error, ..
                    } => {
                        let status = if *is_error { "failed" } else { "success" };
                        let _ = repository
                            .update_run_step(
                                id,
                                status,
                                Some(&serde_json::json!(output)),
                                Utc::now().timestamp_millis(),
                            )
                            .await;
                    }
                    _ => {}
                }
                let _ = event_tx_clone.send(event).await;
            }
        });
        let event_tx = recorded_tx;

        let _lock = session.chat_lock.lock().await;

        self.sessions
            .append_message(
                session_id,
                Role::User,
                vec![ContentBlock::Text {
                    text: input.to_string(),
                }],
            )
            .await?;

        let token = CancellationToken::new();
        session.set_cancellation_token(token.clone());

        let history = session.get_history();
        let history_for_turn: Arc<Vec<Message>> = Arc::new(history[..history.len() - 1].to_vec());

        // 获取当前活跃 agent
        let agent_id = session.get_active_agent();
        let agent_descriptor = self
            .agent_registry
            .get(&agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;

        // 渐进切换策略（Phase 3 G11）
        let use_turn_context = self.agent.config.use_turn_context;
        if use_turn_context {
            // 预加载项目上下文（R2 修复）
            let project_context = load_project_context_with_config_async(
                &self.agent.config.workspace,
                self.agent.config.project_context_file.as_deref(),
            )
            .await;

            // 新路径：prepare_turn + run_turn_with_context
            let mut prompt_config = PromptConfig::new(
                agent_descriptor.id.clone(),
                agent_descriptor.system_prompt_base.clone(),
                self.agent.config.workspace.clone(),
            )
            .with_project_context_path_opt(self.agent.config.project_context_file.clone())
            .with_workflow_prompt_path(self.agent.config.prompts_dir.join("workflow-stages.md"))
            .with_template_vars(agent_descriptor.initial_template_vars.clone());

            if let Some(env) = &self.agent.config.initial_env_snapshot {
                prompt_config = prompt_config.with_environment(env.clone());
            }

            if let Some(content) = project_context {
                prompt_config = prompt_config.with_project_context_content(content);
            }

            let turn_ctx = self.agent.prepare_turn(input, history_for_turn, &prompt_config)?;

            // Phase C: Capture snapshot
            let snapshot = super::snapshot_assembler::RuntimeSnapshotAssembler::turn_context_to_snapshot(
                turn_id.clone(),
                &turn_ctx,
            );
            // We use Value for storage to avoid deep coupling
            let snapshot_internal = LastTurnSnapshot {
                turn_id: snapshot.turn_id.clone(),
                prepared_at: snapshot.prepared_at,
                prompt_preview: snapshot
                    .prompt_preview
                    .as_ref()
                    .map(|p| serde_json::to_value(p).unwrap()),
                tools: snapshot
                    .tools
                    .iter()
                    .map(|t| serde_json::to_value(t).unwrap())
                    .collect(),
                skills: snapshot
                    .skills
                    .iter()
                    .map(|s| serde_json::to_value(s).unwrap())
                    .collect(),
                memory_hits: None,
                usage: None,
            };
            self.sessions
                .update_runtime_state(session_id, Some(snapshot_internal), None)
                .await?;

            let user_message = Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: input.to_string(),
                }],
            };
            let turn_result = match self
                .agent
                .run_turn_with_context(turn_ctx, user_message, event_tx, Some(token))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    self.sessions
                        .get_repository()
                        .update_run_status(&run_id, "failed", Utc::now().timestamp_millis())
                        .await?;
                    return Err(e);
                }
            };

            for msg in &turn_result.messages {
                self.sessions
                    .append_message(session_id, msg.role.clone(), msg.content.clone())
                    .await?;
            }

            // Phase C: Update usage
            let usage = &turn_result.usage;
            self.sessions
                .update_runtime_state(
                    session_id,
                    None,
                    Some((
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.cache_creation_input_tokens,
                        usage.cache_read_input_tokens,
                    )),
                )
                .await?;

            // Phase 2: Update Run status
            self.sessions
                .get_repository()
                .update_run_status(&run_id, "success", Utc::now().timestamp_millis())
                .await?;

            session.clear_cancellation_token();
            session.touch_updated_at();
            Ok(turn_result)
        } else {
            // 旧路径：run_turn（默认）
            let history_for_turn: &[Message] = &history[..history.len() - 1];
            let turn_result = match self
                .agent
                .run_turn(history_for_turn, input, event_tx, Some(token))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    self.sessions
                        .get_repository()
                        .update_run_status(&run_id, "failed", Utc::now().timestamp_millis())
                        .await?;
                    return Err(e);
                }
            };

            for msg in &turn_result.messages {
                self.sessions
                    .append_message(session_id, msg.role.clone(), msg.content.clone())
                    .await?;
            }

            // Phase C: Update usage
            let usage = &turn_result.usage;
            self.sessions
                .update_runtime_state(
                    session_id,
                    None,
                    Some((
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.cache_creation_input_tokens,
                        usage.cache_read_input_tokens,
                    )),
                )
                .await?;

            // Phase 2: Update Run status
            self.sessions
                .get_repository()
                .update_run_status(&run_id, "success", Utc::now().timestamp_millis())
                .await?;

            session.clear_cancellation_token();
            session.touch_updated_at();
            Ok(turn_result)
        }
    }
}
