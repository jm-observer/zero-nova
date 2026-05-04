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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
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

    pub async fn switch_agent(
        &self,
        _session_id: &str,
        agent_id: &str,
    ) -> Result<(AgentDescriptor, Arc<crate::conversation::session::Session>)> {
        let agent = self
            .agent_registry
            .get(agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;

        if let Some(session) = self.sessions.find_latest_session_by_agent(agent_id).await? {
            let session = self.sessions.touch_session(&session.id).await?;
            return Ok((agent, session));
        }

        let session = self
            .sessions
            .create_for_agent(None, agent_id.to_string(), agent.system_prompt_template.clone(), None)
            .await?;

        Ok((agent, session))
    }

    pub async fn set_project_dir(&self, session_id: &str, path: &Path) -> Result<PathBuf> {
        self.sessions.set_project_dir(session_id, path).await
    }

    pub async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        self.sessions.get_project_dir(session_id).await
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
        let observed_skills = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let observed_skills_for_task = observed_skills.clone();

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
                    crate::event::AgentEvent::SkillActivated {
                        skill_id, skill_name, ..
                    } => {
                        log::info!("[SKILL_REC] Observed SkillActivated: {} ({})", skill_name, skill_id);
                        observed_skills_for_task.lock().await.push(serde_json::json!({
                            "skill_id": skill_id,
                            "skillId": skill_id,
                            "name": skill_name,
                            "display_name": skill_name,
                            "status": "active",
                            "description": serde_json::Value::Null
                        }));
                    }
                    crate::event::AgentEvent::SkillSwitched { to_skill, .. } => {
                        log::info!("[SKILL_REC] Observed SkillSwitched to: {}", to_skill);
                        observed_skills_for_task.lock().await.push(serde_json::json!({
                            "skill_id": to_skill,
                            "skillId": to_skill,
                            "name": to_skill,
                            "display_name": to_skill,
                            "status": "active",
                            "description": serde_json::Value::Null
                        }));
                    }
                    crate::event::AgentEvent::SkillExited { skill_id, .. } => {
                        log::info!("[SKILL_REC] Observed SkillExited: {}", skill_id);
                        observed_skills_for_task.lock().await.push(serde_json::json!({
                            "skill_id": skill_id,
                            "name": skill_id,
                            "status": "exited",
                            "description": serde_json::Value::Null
                        }));
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
                None,
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
            let project_dir = self.sessions.get_project_dir(session_id).await?;
            let project_context = load_project_context_with_config_async(
                project_dir.as_deref(),
                self.agent.config.project_context_file.as_deref(),
            )
            .await;

            // 新路径：prepare_turn + run_turn_with_context
            let mut prompt_config = PromptConfig::new(
                agent_descriptor.id.clone(),
                agent_descriptor.system_prompt_base.clone(),
                project_dir.clone(),
            )
            .with_project_context_path_opt(self.agent.config.project_context_file.clone())
            .with_workflow_prompt_path(self.agent.config.prompts_dir.join("workflow-stages.md"))
            .with_template_vars(agent_descriptor.initial_template_vars.clone());

            let mut env =
                crate::prompt::EnvironmentSnapshot::collect(&self.agent.config.config_dir, project_dir.as_deref())
                    .await;
            env.model_id = self
                .agent
                .config
                .initial_env_snapshot
                .as_ref()
                .and_then(|e| e.model_id.clone());
            prompt_config = prompt_config.with_environment(env.clone());

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
            let initial_skills =
                self.collect_current_skills(turn_ctx.active_skill.as_ref().map(|s| s.skill_id.as_str()));
            self.sessions
                .update_runtime_state(session_id, Some(snapshot_internal.clone()), None, Some(initial_skills))
                .await?;

            let user_message = Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: input.to_string(),
                }],
                Utc::now().timestamp_millis(),
            );
            let active_skill_id = turn_ctx.active_skill.as_ref().map(|s| s.skill_id.clone());
            let turn_result = match self
                .agent
                .run_turn_with_context(turn_ctx, user_message, session_id, Some(env), event_tx, Some(token))
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
                let metadata = if msg.role == Role::Assistant {
                    turn_result
                        .provider_request_body
                        .as_ref()
                        .zip(turn_result.provider_response_body.as_ref())
                        .map(|(request_body, response_body)| {
                            serde_json::json!({
                                "providerHttpTrace": {
                                    "requestBody": request_body,
                                    "responseBody": response_body,
                                    "format": "json",
                                    "boundMessageId": "",
                                    "capturedAt": Utc::now().timestamp_millis(),
                                    "truncated": false
                                }
                            })
                        })
                } else {
                    None
                };
                self.sessions
                    .append_message(session_id, msg.role.clone(), msg.content.clone(), metadata)
                    .await?;
            }

            // Phase C: Update usage and skills
            let usage = &turn_result.usage;
            let mut final_skills = self.collect_current_skills(active_skill_id.as_deref());
            {
                // 合并运行过程中观察到的技能（动态激活/切换/退出事件）
                let observed = observed_skills.lock().await;
                log::info!(
                    "[SKILL_REC] Merging {} observed events into {} initial skills",
                    observed.len(),
                    final_skills.len()
                );
                final_skills.extend(observed.clone());
            }

            log::info!(
                "[SKILL_REC] Final skill list for session {}: {:?}",
                session_id,
                final_skills
            );

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
                    Some(final_skills),
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
                .run_turn(
                    history_for_turn,
                    input,
                    session_id,
                    self.agent.config.initial_env_snapshot.clone(),
                    event_tx,
                    Some(token),
                )
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
                let metadata = if msg.role == Role::Assistant {
                    turn_result
                        .provider_request_body
                        .as_ref()
                        .zip(turn_result.provider_response_body.as_ref())
                        .map(|(request_body, response_body)| {
                            serde_json::json!({
                                "providerHttpTrace": {
                                    "requestBody": request_body,
                                    "responseBody": response_body,
                                    "format": "json",
                                    "boundMessageId": "",
                                    "capturedAt": Utc::now().timestamp_millis(),
                                    "truncated": false
                                }
                            })
                        })
                } else {
                    None
                };
                self.sessions
                    .append_message(session_id, msg.role.clone(), msg.content.clone(), metadata)
                    .await?;
            }

            // Phase C: Update usage and skills
            let usage = &turn_result.usage;
            let mut final_skills = self.collect_current_skills(None);
            {
                let observed = observed_skills.lock().await;
                final_skills.extend(observed.clone());
            }

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
                    Some(final_skills),
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

    fn collect_current_skills(&self, active_skill_id: Option<&str>) -> Vec<serde_json::Value> {
        let mut skills = Vec::new();
        if let Some(ref registry) = self.agent.skill_registry {
            for pkg in &registry.packages {
                let status = if active_skill_id == Some(&pkg.id) {
                    "active"
                } else {
                    "available"
                };
                skills.push(serde_json::json!({
                    "skill_id": pkg.id,
                    "skillId": pkg.id,
                    "name": pkg.display_name,
                    "display_name": pkg.display_name,
                    "status": status,
                    "description": pkg.description
                }));
            }
        }
        skills
    }
}
