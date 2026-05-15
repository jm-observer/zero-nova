use crate::agent::{AgentRuntime, TurnResult, TurnWithContextRequest};
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::app::agent_registry_snapshot::AgentRegistrySnapshot;
use crate::app::config_snapshot::ConfigSnapshot;
use crate::conversation::control::{LastTurnSnapshot, ModelRef};
use crate::conversation::model::{RunRecord, RunStepRecord};
use crate::conversation::SessionService;
use crate::event::AgentEvent;
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::{
    ProjectInstructionProfile, PromptConstructionRequest, PromptExtraSections, SkillInjectionMode, SystemPromptBuilder,
    ToolGuidanceMode,
};
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use nova_protocol::observability::{TurnUsage, UsageCompleteness, UsageSource};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// 核心会话业务服务
pub struct ConversationService<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub agent_registry: Arc<dyn AgentRegistrySnapshot>,
    pub sessions: SessionService,
    pub config_snapshot: Arc<dyn ConfigSnapshot>,
    turn_prompt_loader: Arc<dyn TurnPromptMaterialLoader>,
}

#[async_trait]
pub trait TurnPromptMaterialLoader: Send + Sync {
    async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<crate::prompt::TurnPromptMaterial>;
}

impl<C: LlmClient + 'static> ConversationService<C> {
    #[allow(dead_code)]
    fn parse_project_instruction_profile(raw: &str) -> ProjectInstructionProfile {
        match raw {
            "analysis" => ProjectInstructionProfile::Analysis,
            "code" => ProjectInstructionProfile::Code,
            "design" => ProjectInstructionProfile::Design,
            "review" => ProjectInstructionProfile::Review,
            "full" => ProjectInstructionProfile::Full,
            _ => ProjectInstructionProfile::Auto,
        }
    }

    fn parse_skill_injection(raw: &str) -> SkillInjectionMode {
        match raw {
            "active_full" => SkillInjectionMode::ActiveFull,
            "full" => SkillInjectionMode::Full,
            _ => SkillInjectionMode::Catalog,
        }
    }

    #[allow(dead_code)]
    fn parse_tool_guidance(raw: &str) -> ToolGuidanceMode {
        match raw {
            "full" => ToolGuidanceMode::Full,
            _ => ToolGuidanceMode::Compact,
        }
    }

    pub fn new(
        agent: AgentRuntime<C>,
        agent_registry: AgentRegistry,
        sessions: SessionService,
        config_snapshot: Arc<dyn ConfigSnapshot>,
        turn_prompt_loader: Arc<dyn TurnPromptMaterialLoader>,
    ) -> Self {
        Self::new_with_registry_snapshot(
            agent,
            Arc::new(StaticAgentRegistrySnapshot {
                registry: agent_registry,
            }),
            sessions,
            config_snapshot,
            turn_prompt_loader,
        )
    }

    pub fn new_with_registry_snapshot(
        agent: AgentRuntime<C>,
        agent_registry: Arc<dyn AgentRegistrySnapshot>,
        sessions: SessionService,
        config_snapshot: Arc<dyn ConfigSnapshot>,
        turn_prompt_loader: Arc<dyn TurnPromptMaterialLoader>,
    ) -> Self {
        Self {
            agent,
            agent_registry,
            sessions,
            config_snapshot,
            turn_prompt_loader,
        }
    }

    async fn resolve_run_models(
        &self,
        session: &crate::conversation::session::Session,
        agent_descriptor: &AgentDescriptor,
        app_config: &crate::config::AppConfig,
    ) -> Result<(Option<ModelRef>, Option<ModelRef>, crate::config::ResolvedAgentBinding)> {
        let control = session.control.read().await;
        let base_binding = app_config.resolve_agent_binding_by_id(agent_descriptor.id.as_str())?;
        let default_model = ModelRef {
            provider: base_binding.provider_id.clone(),
            model: base_binding.model_config.model.clone(),
        };

        let orchestration_model = control
            .model_override
            .orchestration
            .clone()
            .or(Some(default_model.clone()));
        let execution_model = control.model_override.execution.clone().or(Some(default_model));

        Ok((orchestration_model, execution_model, base_binding))
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
        if let Some(token) = session.take_cancellation_token().await {
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
            .current()
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

        let session = self
            .sessions
            .get_with_history(session_id)
            .await?
            .context("Session not found")?;
        let agent_id = session.get_active_agent().await;
        let agent_descriptor = self
            .agent_registry
            .current()
            .get(&agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;
        let app_config = self.config_snapshot.current().await;
        let (orchestration_model, execution_model, base_binding) = self
            .resolve_run_models(&session, &agent_descriptor, &app_config)
            .await?;

        // Phase 2: Create Run record
        self.sessions
            .get_repository()
            .create_run(&RunRecord {
                id: run_id.clone(),
                session_id: session_id.to_string(),
                status: "running".to_string(),
                created_at: now,
                updated_at: now,
                orchestration_model: orchestration_model.clone(),
                execution_model: execution_model.clone(),
                tool_call_count: Some(0),
                usage: None,
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
        session.set_cancellation_token(token.clone()).await;

        let history = session.get_history().await;
        let history_for_turn: Arc<Vec<Message>> = Arc::new(history[..history.len() - 1].to_vec());

        // 获取当前活跃 agent
        let agent_id = session.get_active_agent().await;
        let agent_descriptor = self
            .agent_registry
            .current()
            .get(&agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;

        let execution_binding = if let Some(override_model) = execution_model.as_ref() {
            app_config.resolve_model_override(
                &base_binding,
                override_model.provider.as_str(),
                override_model.model.as_str(),
            )?
        } else {
            base_binding.clone()
        };
        let project_dir = self.sessions.get_project_dir(session_id).await?;

        // 新路径：prepare_turn + run_turn_with_context
        let system_prompt_base = {
            let control = session.control.read().await;
            control
                .system_prompt_base_override
                .clone()
                .unwrap_or_else(|| agent_descriptor.system_prompt_base.clone())
        };
        let compaction = app_config.prompt_compaction.clone();

        let mut env =
            crate::prompt::EnvironmentSnapshot::collect(&self.agent.config.config_dir, project_dir.as_deref()).await;
        env.model_id = Some(execution_binding.model_config.model.clone());
        let mut context_overrides = HashMap::new();
        context_overrides.insert("workflow_stage".to_string(), "idle".to_string());
        context_overrides.insert("pending_interaction".to_string(), "none".to_string());
        context_overrides.insert("active_agent".to_string(), agent_descriptor.display_name.clone());
        let active_skill_id = self.agent.resolve_active_skill_id(input, history_for_turn.as_ref())?;

        // 使用统一构建管道：构建 PromptConstructionRequest → build_from_request
        let injection_mode = if compaction.enabled {
            Self::parse_skill_injection(compaction.skill_injection.as_str())
        } else {
            SkillInjectionMode::Full
        };

        let prepared_tool_context = self
            .agent
            .prepare_turn(input, history_for_turn.clone(), String::new())
            .await?;
        let visible_tool_names: HashSet<String> = prepared_tool_context
            .tool_definitions
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        let request = PromptConstructionRequest {
            base_material_id: agent_descriptor.id.clone(),
            base_prompt: system_prompt_base.clone(),
            skill_id: active_skill_id.clone(),
            injection_mode,
            initial_template_vars: agent_descriptor.initial_template_vars.clone(),
            context_overrides: context_overrides.clone(),
            original_base_user_message: Some(input.to_string()),
            tool_definitions: Arc::new(prepared_tool_context.tool_definitions.clone()),
            visible_tool_names: Arc::new(visible_tool_names),
            project_instruction_profile: if compaction.enabled {
                Self::parse_project_instruction_profile(compaction.project_instruction_profile.as_str())
            } else {
                ProjectInstructionProfile::Full
            },
            tool_guidance: if compaction.enabled {
                Self::parse_tool_guidance(compaction.tool_guidance.as_str())
            } else {
                ToolGuidanceMode::Full
            },
            agent_catalog: None,
        };

        // 额外 sections：保留旧的 turn_prompt_loader 调用以获取 developer/project/workflow sections
        let turn_material = self
            .turn_prompt_loader
            .load_turn_material(
                project_dir.as_deref(),
                Some("idle"),
                active_skill_id.clone(),
                context_overrides,
                agent_descriptor.enable_project_developer_prompt,
            )
            .await?;
        if let Some(ref content) = turn_material.developer_project_prompt {
            let file_count = content.matches("### Source:").count();
            log::info!("Loaded developer project prompt for turn: {} files matched", file_count);
        }

        let extra_sections = PromptExtraSections {
            system_prompt_base: Some(system_prompt_base),
            developer_project_prompt: turn_material.developer_project_prompt,
            project_context: turn_material.project_context,
            workflow_prompt: turn_material.workflow_prompt,
            environment_snapshot: Some(env.clone()),
        };

        let fallback_skill_registry = crate::skill::SkillRegistry::new();
        let skill_registry = self.agent.skill_registry.as_deref().unwrap_or(&fallback_skill_registry);
        let name_overrides: HashMap<String, String> = HashMap::new();
        let system_prompt = SystemPromptBuilder::default().build_from_request(
            &request,
            &name_overrides,
            skill_registry,
            extra_sections,
        );
        let turn_ctx = self.agent.prepare_turn(input, history_for_turn, system_prompt).await?;

        // Phase C: Capture snapshot
        let snapshot =
            super::snapshot_assembler::RuntimeSnapshotAssembler::turn_context_to_snapshot(turn_id.clone(), &turn_ctx);
        // We use Value for storage to avoid deep coupling
        let prompt_preview_value = snapshot
            .prompt_preview
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .context("failed to serialize prompt preview for snapshot")?;
        let tools: Vec<serde_json::Value> = snapshot
            .tools
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .context("failed to serialize tools for snapshot")?;
        let skills: Vec<serde_json::Value> = snapshot
            .skills
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .context("failed to serialize skills for snapshot")?;
        let snapshot_internal = LastTurnSnapshot {
            turn_id: snapshot.turn_id.clone(),
            prepared_at: snapshot.prepared_at,
            prompt_preview: prompt_preview_value,
            tools,
            skills,
            memory_hits: None,
            usage: None,
        };
        let initial_skills = self.collect_current_skills(turn_ctx.active_skill.as_ref().map(|s| s.skill_id.as_str()));
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
        let execution_model_config: crate::provider::ModelConfig = execution_binding.model_config.clone().into();
        let turn_result = match self
            .agent
            .run_turn_with_context_and_model_config(TurnWithContextRequest {
                ctx: turn_ctx,
                message: user_message,
                session_id,
                agent_id: Some(&agent_id),
                environment: Some(env),
                event_tx,
                cancellation_token: Some(token),
                model_config: &execution_model_config,
            })
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
        let turn_usage = TurnUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
            source: UsageSource::Provider,
            completeness: infer_usage_completeness(usage),
            raw_provider_usage: usage.raw_provider_usage.clone(),
        };
        let turn_usage_value = serde_json::to_value(&turn_usage)?;
        self.sessions
            .get_repository()
            .update_run_usage(&run_id, &turn_usage_value)
            .await?;
        let mut final_skills = self.collect_current_skills(active_skill_id.as_deref());
        {
            // 合并运行过程中观察到的技能（动态激�?切换/退出事件）
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
                Some(LastTurnSnapshot {
                    usage: Some(turn_usage_value),
                    ..snapshot_internal
                }),
                Some((
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_creation_input_tokens.unwrap_or(0),
                    usage.cache_read_input_tokens.unwrap_or(0),
                )),
                Some(final_skills),
            )
            .await?;

        // Phase 2: Update Run status
        self.sessions
            .get_repository()
            .update_run_status(&run_id, "success", Utc::now().timestamp_millis())
            .await?;

        session.clear_cancellation_token().await;
        session.touch_updated_at();
        Ok(turn_result)
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

struct StaticAgentRegistrySnapshot {
    registry: AgentRegistry,
}

impl AgentRegistrySnapshot for StaticAgentRegistrySnapshot {
    fn current(&self) -> AgentRegistry {
        self.registry.clone()
    }
}

fn infer_usage_completeness(usage: &crate::provider::types::Usage) -> UsageCompleteness {
    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        return UsageCompleteness::Missing;
    }
    if usage.cache_creation_input_tokens.is_some() || usage.cache_read_input_tokens.is_some() {
        UsageCompleteness::Full
    } else {
        UsageCompleteness::Partial
    }
}
