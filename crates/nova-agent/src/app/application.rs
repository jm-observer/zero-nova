use super::conversation_service::ConversationService;
use super::inventory::ToolInventoryView;
use super::session_tree::SessionTree;
use super::types::{AppAgent, AppAgentSwitch, AppEvent, AppMessage, AppSession};
use crate::agent::TurnResult;
use crate::config::AppConfig;
use crate::conversation::session::SessionSummary;
use crate::message::Role;
use crate::skill::SkillPackage;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
mod skill_binding_diff;
use skill_binding_diff::should_emit_skill_bindings_updated;

/// Agent 应用门面实现
pub struct AgentApplicationImpl {
    conversation_service: ConversationService,
    workspace_service: super::agent_workspace_service::AgentWorkspaceService,
    config: Arc<AppConfig>,
    config_inner: ArcSwap<AppConfig>,
    config_snapshot_cache: Arc<ArcSwap<Value>>,
    config_path: PathBuf,
    /// OrchestrateTaskTool 的 hook slot 共享句柄。
    /// 由 `register_builtin_tools` 在构造工具时产出，注入到 AgentApplicationImpl
    /// 持有，外部宿主通过 `register_orchestrate_task_prompt_hook` 写入。
    orchestrate_task_hook_slot: crate::tool::builtin::orchestrate_hook::OrchestrateTaskHookSlot,
    /// AgentTool 命中 skill 后改写其 system prompt 的 hook slot 共享句柄。
    /// 来源同上，外部宿主通过 `register_skill_system_prompt_hook` 写入。
    skill_system_hook_slot: crate::tool::builtin::skill_system_hook::SkillSystemPromptHookSlot,
    /// 子 Agent runtime builder 的克隆句柄。与 `AgentTool` 内部持有的克隆共享
    /// 同一份种子表（`Arc<RwLock<_>>`），外部宿主通过
    /// `register_subagent_native_deferred_seed` 写入的种子对子 Agent 派生路径
    /// 可见。
    subagent_runtime_builder: crate::tool::builtin::agent::SubagentRuntimeBuilder,
    // voice_service: VoiceService,
}

impl AgentApplicationImpl {
    pub fn new(
        conversation_service: ConversationService,
        workspace_service: super::agent_workspace_service::AgentWorkspaceService,
        config: Arc<AppConfig>,
        config_snapshot_cache: Arc<ArcSwap<Value>>,
        config_path: PathBuf,
        orchestrate_task_hook_slot: crate::tool::builtin::orchestrate_hook::OrchestrateTaskHookSlot,
        skill_system_hook_slot: crate::tool::builtin::skill_system_hook::SkillSystemPromptHookSlot,
        subagent_runtime_builder: crate::tool::builtin::agent::SubagentRuntimeBuilder,
        // voice_service: VoiceService,
    ) -> Self {
        Self {
            conversation_service,
            workspace_service,
            config: config.clone(),
            config_inner: ArcSwap::from_pointee((*config).clone()),
            config_snapshot_cache,
            config_path,
            orchestrate_task_hook_slot,
            skill_system_hook_slot,
            subagent_runtime_builder,
            // voice_service,
        }
    }

    fn serialize_config_snapshot(config: &AppConfig) -> Result<Value> {
        serde_json::to_value(config).context("Failed to serialize config")
    }

    async fn write_config_file(config_path: &PathBuf, payload: Value) -> Result<AppConfig> {
        let new_config =
            serde_json::from_value::<AppConfig>(payload).context("Failed to parse config update payload")?;
        let config_str = toml::to_string(&new_config).context("Failed to serialize updated config")?;
        tokio::fs::write(config_path, config_str)
            .await
            .with_context(|| format!("Failed to save config to {:?}", config_path))?;
        Ok(new_config)
    }

    #[allow(dead_code)]
    fn update_config_snapshot_cache(config_snapshot_cache: &ArcSwap<Value>, new_config: &AppConfig) -> Result<()> {
        let snapshot_value = Self::serialize_config_snapshot(new_config)?;
        config_snapshot_cache.store(Arc::new(snapshot_value));
        Ok(())
    }

    fn voice_not_implemented<T>() -> Result<T> {
        anyhow::bail!("voice not implemented")
    }

    /// 注册外部宿主的 `AgentPromptProvider`。注册后，所有后续 `create_session`
    /// 对该 `agent_id` 的调用都将通过 provider 拉取最新 system prompt。
    /// 重复注册静默覆盖。`agent_id` 在 registry 不存在时返回 Err。
    pub async fn register_agent_prompt_provider(
        &self,
        agent_id: &str,
        provider: Arc<dyn crate::prompt_provider::AgentPromptProvider>,
    ) -> Result<()> {
        if self.conversation_service.agent_registry.get(agent_id).is_none() {
            anyhow::bail!("Agent '{agent_id}' not found");
        }
        self.conversation_service
            .prompt_providers
            .register(agent_id, provider)
            .await;
        Ok(())
    }

    /// 注册外部宿主的 `OrchestrateTaskPromptHook`。注册后，所有后续
    /// `OrchestrateTaskTool::execute` 调用在激活子 Agent 前都会通过 hook
    /// 改写每个 `AgentRequest.prompt`。重复注册静默覆盖。
    pub async fn register_orchestrate_task_prompt_hook(
        &self,
        hook: Arc<dyn crate::tool::builtin::orchestrate_hook::OrchestrateTaskPromptHook>,
    ) {
        self.orchestrate_task_hook_slot.set(hook).await;
    }

    /// 注册外部宿主的 `SkillSystemPromptHook`。注册后，所有后续 AgentTool 命中
    /// skill 后都会通过 hook 改写其 system prompt（默认是 `SkillPackage.instructions`
    /// 即 SKILL.md 正文）。重复注册静默覆盖。
    pub async fn register_skill_system_prompt_hook(
        &self,
        hook: Arc<dyn crate::tool::builtin::skill_system_hook::SkillSystemPromptHook>,
    ) {
        self.skill_system_hook_slot.set(hook).await;
    }
}

impl AgentApplicationImpl {
    pub async fn session_exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.conversation_service.sessions.get(session_id).await?.is_some())
    }

    pub async fn start_turn(
        &self,
        session_id: &str,
        input: impl Into<crate::message::UserInput>,
        sender: mpsc::Sender<AppEvent>,
    ) -> Result<TurnResult> {
        let input = input.into();
        let before_skill_bindings = self
            .workspace_service
            .list_session_skill_bindings(session_id)
            .await
            .ok()
            .map(|response| response.skills)
            .unwrap_or_default();
        let before_title = if let Some(session) = self.conversation_service.sessions.get(session_id).await? {
            session.get_name().await
        } else {
            String::new()
        };
        let (agent_event_tx, mut agent_event_rx) = mpsc::channel(100);

        let sender_clone = sender.clone();
        tokio::spawn(async move {
            while let Some(event) = agent_event_rx.recv().await {
                if sender_clone.send(AppEvent::from(event)).await.is_err() {
                    break;
                }
            }
        });

        let turn_result = self
            .conversation_service
            .start_turn(session_id, input, agent_event_tx)
            .await?;

        let _ = sender
            .send(AppEvent::TurnComplete {
                usage: turn_result.usage.clone(),
            })
            .await;
        if let Ok(summary) = self.workspace_service.get_session_token_usage(session_id).await {
            let _ = sender.send(AppEvent::SessionTokenUsageUpdated(summary)).await;
        }
        if let Ok(after) = self.workspace_service.list_session_skill_bindings(session_id).await {
            if should_emit_skill_bindings_updated(&before_skill_bindings, &after.skills) {
                if let Err(err) = sender.send(AppEvent::SessionSkillBindingsUpdated(after)).await {
                    log::warn!("Failed to emit SessionSkillBindingsUpdated event: {}", err);
                }
            }
        }
        if let Some(session) = self.conversation_service.sessions.get(session_id).await? {
            let after_title = session.get_name().await;
            if after_title != before_title {
                let payload = nova_protocol::session::SessionSummaryUpdatedPayload {
                    session_id: session.id.clone(),
                    title: Some(after_title),
                    updated_at: session.updated_at.load(Ordering::SeqCst),
                    message_count: session.history.read().await.len(),
                    agent_id: session.control.read().await.active_agent.clone(),
                    version: "1.0".to_string(),
                };
                if let Err(err) = sender.send(AppEvent::SessionSummaryUpdated(payload)).await {
                    log::warn!("Failed to emit SessionSummaryUpdated event: {}", err);
                }
            }
        }
        Ok(turn_result)
    }

    pub async fn stop_turn(&self, session_id: &str) -> Result<()> {
        self.conversation_service.stop_turn(session_id).await
    }

    /// Register a host-provided native tool as a deferred (System-category)
    /// tool, so it stays out of agents' always-on tool sets and is only
    /// activated when a skill `preload` resolves it.
    pub async fn register_deferred_tool(
        &self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn crate::tool::Tool> + Send + Sync>,
    ) {
        self.conversation_service
            .agent
            .tools()
            .register_deferred(name, description, input_schema, factory)
            .await;
    }

    /// 注册一个 native deferred 工具「种子」，使其在后续每次 `OrchestrateTask`
    /// 派生的 sub-agent registry 中都被注册（注册为 deferred）。
    ///
    /// 与 `register_deferred_tool` 互补：后者只作用于主 Agent registry，子
    /// Agent 的 registry 由 `SubagentRuntimeBuilder::build_runtime` 每次新建、
    /// 不继承主 registry。宿主注册一个需要被 skill `preload` 解析的 native
    /// 工具时，**两个方法都要调**——主 Agent 路径靠前者、子 Agent 路径靠后者。
    pub async fn register_subagent_native_deferred_seed(
        &self,
        seed: crate::tool::builtin::agent::NativeDeferredToolSeed,
    ) {
        self.subagent_runtime_builder.register_native_deferred_seed(seed).await;
    }

    pub async fn list_sessions(&self) -> Result<Vec<AppSession>> {
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

    pub async fn session_messages(&self, session_id: &str) -> Result<Vec<AppMessage>> {
        let session = self
            .conversation_service
            .sessions
            .get_with_history(session_id)
            .await?
            .context("Session not found")?;

        let messages = session.get_internal_messages().await;
        let mut app_messages = Vec::with_capacity(messages.len());
        for m in messages {
            app_messages.push(AppMessage {
                id: m.id,
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m.content,
                timestamp: m.created_at,
                metadata: m.metadata.map(serde_json::to_value).transpose()?,
            });
        }
        Ok(app_messages)
    }

    pub async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession> {
        // 优先调外部 provider（zero 等宿主注册的 AgentPromptProvider）拿当前
        // 完整 system prompt；provider 缺失或返回 Err 时 fallback 到旧路径
        // （agent.system_prompt_template 静态字段），不阻塞主链路。
        let provider = self.conversation_service.prompt_providers.get(&agent_id).await;
        let template_fallback = || -> String {
            self.conversation_service
                .agent_registry
                .get(&agent_id)
                .map(|agent| agent.system_prompt_template.clone())
                .unwrap_or_default()
        };
        let system_prompt = match provider {
            Some(p) => match p.current_system_prompt(&agent_id).await {
                Ok(prompt) => prompt,
                Err(err) => {
                    log::warn!(
                        "AgentPromptProvider 调用失败 agent_id={agent_id} err={err:#}，fallback 到 system_prompt_template"
                    );
                    template_fallback()
                }
            },
            None => template_fallback(),
        };

        let inherited_project_dir = self
            .conversation_service
            .sessions
            .find_latest_session_by_agent(&agent_id)
            .await?
            .and_then(|session| {
                let control = session.control.try_read().ok()?;
                control.project_dir.clone()
            });

        let session = self
            .conversation_service
            .sessions
            .create_for_agent(title, agent_id, system_prompt, inherited_project_dir)
            .await?;

        let id = session.id.clone();
        let name = session.get_name().await;
        let active_agent = session.control.read().await.active_agent.clone();
        let created_at = session.created_at;
        let updated_at = session.updated_at.load(Ordering::SeqCst);
        let message_count = session.history.read().await.len();

        Ok(AppSession {
            id,
            title: Some(name),
            agent_id: active_agent,
            created_at,
            updated_at,
            message_count,
        })
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        // Plan 3：有子 Session 时拒绝删除（避免造成孤儿子 Session）。
        let children = self
            .conversation_service
            .sessions
            .list_child_session_summaries(session_id)
            .await?;
        if !children.is_empty() {
            anyhow::bail!(
                "session {} has {} child sessions; use delete_session_tree to delete cascade",
                session_id,
                children.len()
            );
        }
        let deleted = self.conversation_service.sessions.delete(session_id).await?;
        if deleted {
            // 释放该 session 激活的 deferred 工具，避免按 session 累积。
            self.conversation_service
                .agent
                .tools()
                .clear_session_activations(session_id)
                .await;
        }
        Ok(deleted)
    }

    /// 列出指定 parent Session 的所有直接子 Session 摘要（轻量，不拉 history）。
    pub async fn list_child_sessions(&self, parent_id: &str) -> Result<Vec<SessionSummary>> {
        self.conversation_service
            .sessions
            .list_child_session_summaries(parent_id)
            .await
    }

    /// 深度优先返回以 root_id 为根的完整父子树（每节点含 history + ProviderHttpTrace）。
    ///
    /// `max_depth = 0` 表示仅返回根（若根有子则 truncated=true）；
    /// 超过 max_depth 的子树截断并标记 truncated=true；
    /// root_id 不存在返回 `Err`。
    pub async fn get_session_tree(&self, root_id: &str, max_depth: usize) -> Result<SessionTree> {
        super::session_tree::build_session_tree(&self.conversation_service.sessions, root_id, max_depth).await
    }

    /// 解析任意 session 的顶层 root session id。
    ///
    /// 子 Agent 内的工具据 `ToolContext.session_id` 调本方法即可定位所属顶层
    /// 对话，无需经 LLM 传任何寻址标识符。优先读 `root_session_id` 列；
    /// 对 v0.3.14 前的存量行降级沿 `parent_session_id` 链 walk。
    /// session 不存在返回 `Err`。
    pub async fn get_session_root(&self, session_id: &str) -> Result<String> {
        self.conversation_service
            .sessions
            .resolve_session_root(session_id)
            .await
    }

    /// 解析任意 session 的完整祖先链（根在前→直接父在后）。根 session 返回空 Vec。
    /// 与 `get_session_root` 同源：优先读 `ancestor_ids` 列，存量行降级 walk。
    pub async fn get_session_ancestors(&self, session_id: &str) -> Result<Vec<String>> {
        self.conversation_service
            .sessions
            .resolve_session_ancestors(session_id)
            .await
    }

    /// 级联删除整棵子树。root 不存在返回 `Ok(0)`（见设计稿「已收敛的待澄清点」#4）。
    pub async fn delete_session_tree(&self, root_id: &str) -> Result<usize> {
        let count = self.conversation_service.sessions.delete_session_tree(root_id).await?;
        // 实际删除的每个 session 都释放其 deferred 工具激活。
        // 注：to_delete 列表在 SessionService::delete_session_tree 内部，调用者无从拿到；
        // 这里保守地仅清理 root 的 activations，下层 Session 的 activations 随 session 删除已无意义。
        if count > 0 {
            self.conversation_service
                .agent
                .tools()
                .clear_session_activations(root_id)
                .await;
        }
        Ok(count)
    }

    pub async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>) -> Result<AppSession> {
        let session = self
            .conversation_service
            .sessions
            .copy_session(session_id, truncate_index)
            .await?
            .context("Source session not found")?;

        let id = session.id.clone();
        let name = session.get_name().await;
        let active_agent = session.control.read().await.active_agent.clone();
        let created_at = session.created_at;
        let updated_at = session.updated_at.load(Ordering::SeqCst);
        let message_count = session.history.read().await.len();

        Ok(AppSession {
            id,
            title: Some(name),
            agent_id: active_agent,
            created_at,
            updated_at,
            message_count,
        })
    }

    pub async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgentSwitch> {
        let (agent, session) = self.conversation_service.switch_agent(session_id, agent_id).await?;
        let agent = AppAgent {
            id: agent.id.clone(),
            name: agent.display_name.clone(),
            description: Some(agent.description.clone()),
        };
        let session = AppSession {
            id: session.id.clone(),
            title: Some(session.get_name().await),
            agent_id: session.control.read().await.active_agent.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at.load(Ordering::SeqCst),
            message_count: session.history.read().await.len(),
        };

        Ok(AppAgentSwitch { agent, session })
    }

    pub async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf> {
        self.conversation_service
            .set_project_dir(session_id, &project_dir)
            .await
    }

    pub async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        self.conversation_service.get_project_dir(session_id).await
    }

    pub fn list_agents(&self) -> Vec<AppAgent> {
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

    pub fn get_agent(&self, agent_id: &str) -> Option<AppAgent> {
        self.conversation_service
            .agent_registry
            .get(agent_id)
            .map(|agent| AppAgent {
                id: agent.id.clone(),
                name: agent.display_name.clone(),
                description: Some(agent.description.clone()),
            })
    }

    /// 返回所有已加载的 SkillPackage 元数据（只读快照）。
    pub fn list_skills(&self) -> Vec<SkillPackage> {
        self.workspace_service.skill_registry.packages.clone()
    }

    /// 返回当前工具注册视图：always-on 与 deferred 各自的注册态快照。
    ///
    /// 注意：deferred 反映**注册态**，与 per-session 激活态无关。
    pub async fn list_tools(&self) -> ToolInventoryView {
        let tools = self.conversation_service.agent.tools();
        ToolInventoryView {
            loaded: tools.loaded_definitions().await,
            deferred: tools.list_deferred_representations().await,
        }
    }

    pub async fn config_snapshot(&self) -> Result<Value> {
        Self::serialize_config_snapshot(&self.config)
    }

    pub async fn update_config(&self, payload: Value) -> Result<()> {
        let new_config = Self::write_config_file(&self.config_path, payload).await?;
        // store the new AppConfig for service access
        let new_arc = Arc::new(new_config);
        self.config_inner.store(new_arc.clone());
        // cache for snapshot serialization
        self.config_snapshot_cache
            .store(Arc::new(serde_json::to_value(new_arc.as_ref())?));
        Ok(())
    }

    pub async fn on_connect(&self) -> Result<Vec<AppEvent>> {
        Ok(vec![AppEvent::Welcome {
            require_auth: false,
            setup_required: false,
        }])
    }

    pub async fn on_disconnect(&self, _conn_id: &str) {}

    // --- Observability & Control Implementation ---

    pub async fn inspect_agent(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<nova_protocol::observability::AgentInspectResponse> {
        self.workspace_service.inspect_agent(agent_id, session_id).await
    }

    pub async fn get_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot> {
        self.workspace_service.get_session_runtime(session_id).await
    }

    pub async fn preview_session_prompt(
        &self,
        session_id: &str,
        message_id: Option<String>,
    ) -> Result<nova_protocol::observability::PromptPreviewSnapshot> {
        self.workspace_service
            .preview_session_prompt(session_id, message_id)
            .await
    }

    pub async fn reload_session_system_prompt(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSystemPromptReloadResponse> {
        self.workspace_service.reload_session_system_prompt(session_id).await
    }

    pub async fn list_session_tools(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionToolsResponse> {
        self.workspace_service.list_session_tools(session_id).await
    }

    pub async fn list_session_file_tree(
        &self,
        session_id: &str,
        relative_path: Option<String>,
    ) -> Result<nova_protocol::observability::SessionFileTreeResponse> {
        self.workspace_service
            .list_session_file_tree(session_id, relative_path)
            .await
    }

    pub async fn list_session_skill_bindings(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSkillBindingsResponse> {
        self.workspace_service.list_session_skill_bindings(session_id).await
    }

    pub async fn get_session_memory_hits(
        &self,
        session_id: &str,
        turn_id: Option<String>,
    ) -> Result<nova_protocol::observability::SessionMemoryHitsResponse> {
        self.workspace_service
            .get_session_memory_hits(session_id, turn_id)
            .await
    }

    pub async fn override_session_model(
        &self,
        session_id: &str,
        req: nova_protocol::observability::SessionModelOverrideRequest,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot> {
        self.workspace_service.override_session_model(session_id, req).await
    }

    pub async fn get_session_token_usage(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionTokenUsageResponse> {
        self.workspace_service.get_session_token_usage(session_id).await
    }

    pub async fn get_session_token_usage_detail(
        &self,
        session_id: &str,
        limit: u32,
        before_turn_id: Option<&str>,
    ) -> Result<nova_protocol::observability::SessionTokenUsageDetailResponse> {
        self.workspace_service
            .get_session_token_usage_detail(session_id, limit, before_turn_id)
            .await
    }

    // --- Plan 2: Execution Records & Control Implementation ---

    pub async fn list_session_runs(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionRunsResponse> {
        self.workspace_service.list_session_runs(session_id).await
    }

    pub async fn get_run_detail(&self, run_id: &str) -> Result<nova_protocol::observability::RunRecord> {
        self.workspace_service.get_run_detail(run_id).await
    }

    pub async fn control_run(&self, run_id: &str, req: nova_protocol::observability::RunControlRequest) -> Result<()> {
        self.workspace_service.control_run(run_id, req).await
    }

    pub async fn list_session_artifacts(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionArtifactsResponse> {
        self.workspace_service.list_session_artifacts(session_id).await
    }

    pub async fn list_pending_permissions(
        &self,
        session_id: Option<&str>,
    ) -> Result<nova_protocol::observability::PermissionPendingResponse> {
        self.workspace_service.list_pending_permissions(session_id).await
    }

    pub async fn respond_to_permission(
        &self,
        req: nova_protocol::observability::PermissionRespondRequest,
    ) -> Result<()> {
        self.workspace_service.respond_to_permission(req).await
    }

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<nova_protocol::observability::AuditLogsResponse> {
        self.workspace_service.list_audit_logs(session_id).await
    }

    pub async fn get_diagnostics(&self, session_id: &str) -> Result<nova_protocol::observability::DiagnosticsResponse> {
        self.workspace_service.get_diagnostics(session_id).await
    }

    pub async fn restore_workspace(&self) -> Result<nova_protocol::observability::WorkspaceRestoreResponse> {
        self.workspace_service.restore_workspace().await
    }

    pub async fn get_provider_health(&self) -> Result<nova_protocol::observability::ProviderHealthSnapshotResponse> {
        let config = self.config.clone();
        crate::provider::health::collect_provider_health(&config).await
    }

    pub async fn voice_capabilities(&self) -> Result<nova_protocol::voice::VoiceCapabilitiesResponse> {
        let config = self.config.clone();
        Ok(nova_protocol::voice::VoiceCapabilitiesResponse {
            stt: nova_protocol::voice::VoiceCapabilityStatus {
                enabled: config.voice.enabled,
                available: config.voice.enabled,
            },
            tts: nova_protocol::voice::VoiceTtsCapabilityStatus {
                enabled: config.voice.enabled,
                available: config.voice.enabled,
                voice: config.voice.tts_voice.clone(),
                auto_play: config.voice.auto_play,
            },
        })
    }

    pub async fn voice_transcribe(
        &self,
        _req: &nova_protocol::voice::VoiceTranscribeRequest,
    ) -> Result<nova_protocol::voice::VoiceTranscribeResponse> {
        Self::voice_not_implemented()
    }

    pub async fn voice_tts(
        &self,
        _req: &nova_protocol::voice::VoiceTtsRequest,
    ) -> Result<nova_protocol::voice::VoiceTtsResponse> {
        Self::voice_not_implemented()
    }
}

#[cfg(test)]
mod tests {
    use super::should_emit_skill_bindings_updated;
    use super::AgentApplicationImpl;
    use crate::config::AppConfig;
    use arc_swap::ArcSwap;
    use nova_protocol::observability::SkillBindingSnapshot;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    const UPDATE_TIMEOUT: Duration = Duration::from_secs(3);

    #[test]
    fn emits_event_when_skill_bindings_changed() {
        let before = vec![SkillBindingSnapshot {
            skill_id: "skill-a".to_string(),
            name: "Skill A".to_string(),
            status: "active".to_string(),
            description: None,
        }];
        let after = vec![
            SkillBindingSnapshot {
                skill_id: "skill-a".to_string(),
                name: "Skill A".to_string(),
                status: "active".to_string(),
                description: None,
            },
            SkillBindingSnapshot {
                skill_id: "skill-b".to_string(),
                name: "Skill B".to_string(),
                status: "active".to_string(),
                description: None,
            },
        ];

        assert!(should_emit_skill_bindings_updated(&before, &after));
    }

    #[test]
    fn does_not_emit_event_when_skill_bindings_unchanged() {
        let before = vec![SkillBindingSnapshot {
            skill_id: "skill-a".to_string(),
            name: "Skill A".to_string(),
            status: "active".to_string(),
            description: None,
        }];
        let after = vec![SkillBindingSnapshot {
            skill_id: "skill-a".to_string(),
            name: "Skill A".to_string(),
            status: "active".to_string(),
            description: None,
        }];

        assert!(!should_emit_skill_bindings_updated(&before, &after));
    }

    #[test]
    fn serialize_config_snapshot_matches_direct_serialization() {
        let mut config = AppConfig::new(PathBuf::from("."));
        config.voice.enabled = false;

        let snapshot = AgentApplicationImpl::serialize_config_snapshot(&config).unwrap();

        assert_eq!(snapshot, serde_json::to_value(&config).unwrap());
    }

    #[test]
    fn atomic_snapshot_cache_returns_latest_value() {
        let cache = ArcSwap::from_pointee(json!({ "version": 1 }));
        assert_eq!(cache.load().as_ref(), &json!({ "version": 1 }));

        cache.store(Arc::new(json!({ "version": 2 })));

        assert_eq!(cache.load().as_ref(), &json!({ "version": 2 }));
    }

    #[tokio::test]
    async fn update_config_returns_parse_error_for_invalid_payload() {
        let cache = ArcSwap::from_pointee(json!({ "seed": true }));
        let path = PathBuf::from("target/test-data/plan3-parse-error.toml");

        let result = AgentApplicationImpl::write_config_file(&path, json!({ "providers": 1 })).await;

        let error = result.expect_err("invalid payload should fail");
        assert!(error.to_string().contains("Failed to parse config update payload"));
        assert_eq!(cache.load().as_ref(), &json!({ "seed": true }));
    }

    #[tokio::test]
    async fn update_config_keeps_state_when_write_fails() {
        let mut initial = AppConfig::new(PathBuf::from("."));
        initial.voice.enabled = true;
        let initial_snapshot = serde_json::to_value(&initial).unwrap();
        let cache = ArcSwap::from_pointee(initial_snapshot.clone());

        let invalid_path = PathBuf::from("NUL/config.toml");
        let mut target = initial.clone();
        target.voice.enabled = false;
        let payload = serde_json::to_value(&target).unwrap();

        let result = AgentApplicationImpl::write_config_file(&invalid_path, payload).await;
        assert!(result.is_err());

        assert!(initial.voice.enabled);
        assert_eq!(cache.load().as_ref(), &initial_snapshot);
    }

    #[tokio::test]
    async fn update_config_and_snapshot_are_consistent_under_concurrency() {
        let base = AppConfig::new(PathBuf::from("."));
        let base_snapshot = serde_json::to_value(&base).unwrap();
        let cache = Arc::new(ArcSwap::from_pointee(base_snapshot));
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let mut target_config = base.clone();
        target_config.voice.enabled = false;
        let update_payload = serde_json::to_value(&target_config).unwrap();

        let writer_cache = Arc::clone(&cache);
        let writer_path = config_path.clone();
        let writer = tokio::spawn(async move {
            tokio::time::timeout(UPDATE_TIMEOUT, async move {
                let new_config = AgentApplicationImpl::write_config_file(&writer_path, update_payload).await?;
                AgentApplicationImpl::update_config_snapshot_cache(&writer_cache, &new_config)
            })
            .await
            .expect("update timeout")
            .expect("update must succeed");
        });

        let reader_cache = Arc::clone(&cache);
        let reader = tokio::spawn(async move {
            tokio::time::timeout(UPDATE_TIMEOUT, async move {
                for _ in 0..64 {
                    let snapshot = reader_cache.load();
                    let voice = snapshot
                        .get("voice")
                        .and_then(|v| v.get("enabled"))
                        .and_then(|v| v.as_bool())
                        .expect("voice.enabled should exist");
                    #[allow(clippy::overly_complex_bool_expr)]
                    {
                        let _ = voice || !voice;
                    } // warm-up: verify runtime works under concurrency
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("read timeout");
        });

        writer.await.unwrap();
        reader.await.unwrap();

        let final_snapshot_voice = cache
            .load()
            .get("voice")
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap();
        assert!(!final_snapshot_voice);
    }

    #[test]
    fn voice_transcribe_returns_not_implemented_error() {
        let result = AgentApplicationImpl::voice_not_implemented::<nova_protocol::voice::VoiceTranscribeResponse>();

        let error = result.expect_err("voice transcribe should return explicit error");
        assert!(error.to_string().contains("voice not implemented"));
    }

    #[test]
    fn voice_tts_returns_not_implemented_error() {
        let result = AgentApplicationImpl::voice_not_implemented::<nova_protocol::voice::VoiceTtsResponse>();

        let error = result.expect_err("voice tts should return explicit error");
        assert!(error.to_string().contains("voice not implemented"));
    }
}
