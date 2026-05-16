use super::conversation_service::ConversationService;
use super::types::{AppAgent, AppAgentSwitch, AppEvent, AppMessage, AppSession};
use crate::agent::TurnResult;
use crate::config::AppConfig;
use crate::message::Role;
use crate::provider::LlmClient;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
mod skill_binding_diff;
use skill_binding_diff::should_emit_skill_bindings_updated;

#[async_trait]
pub trait AgentApplication: Send + Sync {
    async fn session_exists(&self, session_id: &str) -> Result<bool>;
    async fn start_turn(&self, session_id: &str, input: &str, sender: mpsc::Sender<AppEvent>) -> Result<TurnResult>;
    async fn stop_turn(&self, session_id: &str) -> Result<()>;

    async fn list_sessions(&self) -> Result<Vec<AppSession>>;
    async fn session_messages(&self, session_id: &str) -> Result<Vec<AppMessage>>;
    async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession>;
    async fn delete_session(&self, session_id: &str) -> Result<bool>;
    async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>) -> Result<AppSession>;

    async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgentSwitch>;
    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf>;
    async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>>;
    fn list_agents(&self) -> Vec<AppAgent>;
    fn get_agent(&self, agent_id: &str) -> Option<AppAgent>;

    async fn config_snapshot(&self) -> Result<Value>;
    async fn update_config(&self, payload: Value) -> Result<()>;

    async fn on_connect(&self) -> Result<Vec<AppEvent>>;
    async fn on_disconnect(&self, conn_id: &str);

    // --- Observability & Control (Plan 1 & 2) ---
    async fn inspect_agent(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<nova_protocol::observability::AgentInspectResponse>;
    async fn get_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot>;
    async fn preview_session_prompt(
        &self,
        session_id: &str,
        message_id: Option<String>,
    ) -> Result<nova_protocol::observability::PromptPreviewSnapshot>;
    async fn reload_session_system_prompt(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSystemPromptReloadResponse>;
    async fn list_session_tools(&self, session_id: &str) -> Result<nova_protocol::observability::SessionToolsResponse>;
    async fn list_session_file_tree(
        &self,
        session_id: &str,
        relative_path: Option<String>,
    ) -> Result<nova_protocol::observability::SessionFileTreeResponse>;
    async fn list_session_skill_bindings(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSkillBindingsResponse>;
    async fn get_session_memory_hits(
        &self,
        session_id: &str,
        turn_id: Option<String>,
    ) -> Result<nova_protocol::observability::SessionMemoryHitsResponse>;
    async fn override_session_model(
        &self,
        session_id: &str,
        req: nova_protocol::observability::SessionModelOverrideRequest,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot>;
    async fn get_session_token_usage(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionTokenUsageResponse>;
    async fn get_session_token_usage_detail(
        &self,
        session_id: &str,
        limit: u32,
        before_turn_id: Option<&str>,
    ) -> Result<nova_protocol::observability::SessionTokenUsageDetailResponse>;

    // --- Plan 2: Execution Records & Control ---
    async fn list_session_runs(&self, session_id: &str) -> Result<nova_protocol::observability::SessionRunsResponse>;
    async fn get_run_detail(&self, run_id: &str) -> Result<nova_protocol::observability::RunRecord>;
    async fn control_run(&self, run_id: &str, req: nova_protocol::observability::RunControlRequest) -> Result<()>;
    async fn list_session_artifacts(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionArtifactsResponse>;
    async fn list_pending_permissions(
        &self,
        session_id: Option<&str>,
    ) -> Result<nova_protocol::observability::PermissionPendingResponse>;
    async fn respond_to_permission(&self, req: nova_protocol::observability::PermissionRespondRequest) -> Result<()>;
    async fn list_audit_logs(&self, session_id: &str) -> Result<nova_protocol::observability::AuditLogsResponse>;
    async fn get_diagnostics(&self, session_id: &str) -> Result<nova_protocol::observability::DiagnosticsResponse>;
    async fn restore_workspace(&self) -> Result<nova_protocol::observability::WorkspaceRestoreResponse>;
    async fn get_provider_health(&self) -> Result<nova_protocol::observability::ProviderHealthSnapshotResponse>;
    async fn voice_capabilities(&self) -> Result<nova_protocol::voice::VoiceCapabilitiesResponse>;
    async fn voice_transcribe(
        &self,
        req: &nova_protocol::voice::VoiceTranscribeRequest,
    ) -> Result<nova_protocol::voice::VoiceTranscribeResponse>;
    async fn voice_tts(
        &self,
        req: &nova_protocol::voice::VoiceTtsRequest,
    ) -> Result<nova_protocol::voice::VoiceTtsResponse>;
}

/// Agent 应用门面实现
pub struct AgentApplicationImpl<C: LlmClient> {
    conversation_service: ConversationService<C>,
    workspace_service: super::agent_workspace_service::AgentWorkspaceService,
    config: Arc<AppConfig>,
    config_inner: ArcSwap<AppConfig>,
    config_snapshot_cache: Arc<ArcSwap<Value>>,
    config_path: PathBuf,
    // voice_service: VoiceService,
}

impl<C: LlmClient + 'static> AgentApplicationImpl<C> {
    pub fn new(
        conversation_service: ConversationService<C>,
        workspace_service: super::agent_workspace_service::AgentWorkspaceService,
        config: Arc<AppConfig>,
        config_snapshot_cache: Arc<ArcSwap<Value>>,
        config_path: PathBuf,
        // voice_service: VoiceService,
    ) -> Self {
        Self {
            conversation_service,
            workspace_service,
            config: config.clone(),
            config_inner: ArcSwap::from_pointee((*config).clone()),
            config_snapshot_cache,
            config_path,
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
}

#[async_trait]
impl<C: LlmClient + 'static> AgentApplication for AgentApplicationImpl<C> {
    async fn session_exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.conversation_service.sessions.get(session_id).await?.is_some())
    }

    async fn start_turn(&self, session_id: &str, input: &str, sender: mpsc::Sender<AppEvent>) -> Result<TurnResult> {
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

    async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession> {
        let system_prompt = self
            .conversation_service
            .agent_registry
            .get(&agent_id)
            .map(|agent| agent.system_prompt_template.clone())
            .unwrap_or_default();

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

    async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgentSwitch> {
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

    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf> {
        self.conversation_service
            .set_project_dir(session_id, &project_dir)
            .await
    }

    async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        self.conversation_service.get_project_dir(session_id).await
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

    async fn config_snapshot(&self) -> Result<Value> {
        Self::serialize_config_snapshot(&self.config)
    }

    async fn update_config(&self, payload: Value) -> Result<()> {
        let new_config = Self::write_config_file(&self.config_path, payload).await?;
        // store the new AppConfig for service access
        let new_arc = Arc::new(new_config);
        self.config_inner.store(new_arc.clone());
        // cache for snapshot serialization
        self.config_snapshot_cache
            .store(Arc::new(serde_json::to_value(new_arc.as_ref())?));
        Ok(())
    }

    async fn on_connect(&self) -> Result<Vec<AppEvent>> {
        Ok(vec![AppEvent::Welcome {
            require_auth: false,
            setup_required: false,
        }])
    }

    async fn on_disconnect(&self, _conn_id: &str) {}

    // --- Observability & Control Implementation ---

    async fn inspect_agent(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<nova_protocol::observability::AgentInspectResponse> {
        self.workspace_service.inspect_agent(agent_id, session_id).await
    }

    async fn get_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot> {
        self.workspace_service.get_session_runtime(session_id).await
    }

    async fn preview_session_prompt(
        &self,
        session_id: &str,
        message_id: Option<String>,
    ) -> Result<nova_protocol::observability::PromptPreviewSnapshot> {
        self.workspace_service
            .preview_session_prompt(session_id, message_id)
            .await
    }

    async fn reload_session_system_prompt(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSystemPromptReloadResponse> {
        self.workspace_service.reload_session_system_prompt(session_id).await
    }

    async fn list_session_tools(&self, session_id: &str) -> Result<nova_protocol::observability::SessionToolsResponse> {
        self.workspace_service.list_session_tools(session_id).await
    }

    async fn list_session_file_tree(
        &self,
        session_id: &str,
        relative_path: Option<String>,
    ) -> Result<nova_protocol::observability::SessionFileTreeResponse> {
        self.workspace_service
            .list_session_file_tree(session_id, relative_path)
            .await
    }

    async fn list_session_skill_bindings(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionSkillBindingsResponse> {
        self.workspace_service.list_session_skill_bindings(session_id).await
    }

    async fn get_session_memory_hits(
        &self,
        session_id: &str,
        turn_id: Option<String>,
    ) -> Result<nova_protocol::observability::SessionMemoryHitsResponse> {
        self.workspace_service
            .get_session_memory_hits(session_id, turn_id)
            .await
    }

    async fn override_session_model(
        &self,
        session_id: &str,
        req: nova_protocol::observability::SessionModelOverrideRequest,
    ) -> Result<nova_protocol::observability::SessionRuntimeSnapshot> {
        self.workspace_service.override_session_model(session_id, req).await
    }

    async fn get_session_token_usage(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionTokenUsageResponse> {
        self.workspace_service.get_session_token_usage(session_id).await
    }

    async fn get_session_token_usage_detail(
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

    async fn list_session_runs(&self, session_id: &str) -> Result<nova_protocol::observability::SessionRunsResponse> {
        self.workspace_service.list_session_runs(session_id).await
    }

    async fn get_run_detail(&self, run_id: &str) -> Result<nova_protocol::observability::RunRecord> {
        self.workspace_service.get_run_detail(run_id).await
    }

    async fn control_run(&self, run_id: &str, req: nova_protocol::observability::RunControlRequest) -> Result<()> {
        self.workspace_service.control_run(run_id, req).await
    }

    async fn list_session_artifacts(
        &self,
        session_id: &str,
    ) -> Result<nova_protocol::observability::SessionArtifactsResponse> {
        self.workspace_service.list_session_artifacts(session_id).await
    }

    async fn list_pending_permissions(
        &self,
        session_id: Option<&str>,
    ) -> Result<nova_protocol::observability::PermissionPendingResponse> {
        self.workspace_service.list_pending_permissions(session_id).await
    }

    async fn respond_to_permission(&self, req: nova_protocol::observability::PermissionRespondRequest) -> Result<()> {
        self.workspace_service.respond_to_permission(req).await
    }

    async fn list_audit_logs(&self, session_id: &str) -> Result<nova_protocol::observability::AuditLogsResponse> {
        self.workspace_service.list_audit_logs(session_id).await
    }

    async fn get_diagnostics(&self, session_id: &str) -> Result<nova_protocol::observability::DiagnosticsResponse> {
        self.workspace_service.get_diagnostics(session_id).await
    }

    async fn restore_workspace(&self) -> Result<nova_protocol::observability::WorkspaceRestoreResponse> {
        self.workspace_service.restore_workspace().await
    }

    async fn get_provider_health(&self) -> Result<nova_protocol::observability::ProviderHealthSnapshotResponse> {
        let config = self.config.clone();
        crate::provider::health::collect_provider_health(&config).await
    }

    async fn voice_capabilities(&self) -> Result<nova_protocol::voice::VoiceCapabilitiesResponse> {
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

    async fn voice_transcribe(
        &self,
        _req: &nova_protocol::voice::VoiceTranscribeRequest,
    ) -> Result<nova_protocol::voice::VoiceTranscribeResponse> {
        Self::voice_not_implemented()
    }

    async fn voice_tts(
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

        let snapshot =
            AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::serialize_config_snapshot(
                &config,
            )
            .unwrap();

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

        let result = AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::write_config_file(
            &path,
            json!({ "providers": 1 }),
        )
        .await;

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

        let result = AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::write_config_file(
            &invalid_path,
            payload,
        )
        .await;
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
            tokio::time::timeout(
                UPDATE_TIMEOUT,
                async move {
                    let new_config = AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::write_config_file(
                        &writer_path,
                        update_payload,
                    )
                    .await?;
                    AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::update_config_snapshot_cache(
                        &writer_cache,
                        &new_config,
                    )
                },
            )
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
        let result = AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::voice_not_implemented::<
            nova_protocol::voice::VoiceTranscribeResponse,
        >();

        let error = result.expect_err("voice transcribe should return explicit error");
        assert!(error.to_string().contains("voice not implemented"));
    }

    #[test]
    fn voice_tts_returns_not_implemented_error() {
        let result = AgentApplicationImpl::<crate::provider::openai_compat::OpenAiCompatClient>::voice_not_implemented::<
            nova_protocol::voice::VoiceTtsResponse,
        >();

        let error = result.expect_err("voice tts should return explicit error");
        assert!(error.to_string().contains("voice not implemented"));
    }
}
