use super::snapshot_assembler::RuntimeSnapshotAssembler;
use crate::agent_catalog::AgentRegistry;
use crate::config::{AppConfig, OriginAppConfig};
use crate::conversation::control::ModelRef;
use crate::conversation::SessionService;
use crate::path_resolver::resolve_path_ref;
use crate::prompt::{PromptConfig, SystemPromptBuilder};
use anyhow::{Context, Result};
use chrono::Utc;
use nova_protocol::observability::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::fs;

pub struct AgentWorkspaceService {
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
    pub config: Arc<RwLock<AppConfig>>,
}

impl AgentWorkspaceService {
    pub fn new(agent_registry: AgentRegistry, sessions: SessionService, config: Arc<RwLock<AppConfig>>) -> Self {
        Self {
            agent_registry,
            sessions,
            config,
        }
    }

    pub async fn inspect_agent(&self, agent_id: &str, session_id: &str) -> Result<AgentInspectResponse> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().unwrap();
        let config = self
            .config
            .read()
            .map_err(|_| anyhow::anyhow!("Application config lock poisoned"))?
            .clone();
        let base_binding = config.resolve_agent_binding_by_id(agent_id)?;

        let default_model = nova_protocol::ModelRef {
            provider: base_binding.provider_id.clone(),
            model: base_binding.model_config.model.clone(),
        };
        let global_default = config.resolve_default_binding()?;
        let has_agent_default = base_binding.provider_id != global_default.provider_id
            || base_binding.model_config.model != global_default.model_config.model;

        let has_session_override =
            control.model_override.orchestration.is_some() || control.model_override.execution.is_some();

        let (orchestration, execution, source) = if has_session_override {
            let orch = control
                .model_override
                .orchestration
                .as_ref()
                .map(|m| nova_protocol::ModelRef {
                    provider: m.provider.clone(),
                    model: m.model.clone(),
                })
                .unwrap_or_else(|| default_model.clone());
            let exec = control
                .model_override
                .execution
                .as_ref()
                .map(|m| nova_protocol::ModelRef {
                    provider: m.provider.clone(),
                    model: m.model.clone(),
                })
                .unwrap_or_else(|| default_model.clone());
            (orch, exec, "session_override".to_string())
        } else if has_agent_default {
            (default_model.clone(), default_model, "agent_default".to_string())
        } else {
            (default_model.clone(), default_model, "global_default".to_string())
        };

        Ok(AgentInspectResponse {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            effective_model: ModelBindingDetailView {
                orchestration,
                execution,
                source,
            },
            updated_at: Utc::now().timestamp_millis(),
        })
    }

    pub async fn get_session_runtime(&self, session_id: &str) -> Result<SessionRuntimeSnapshot> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().unwrap();

        Ok(RuntimeSnapshotAssembler::assemble_session_runtime(session_id, &control))
    }

    pub async fn preview_session_prompt(
        &self,
        session_id: &str,
        _message_id: Option<String>,
    ) -> Result<PromptPreviewSnapshot> {
        let runtime = self.get_session_runtime(session_id).await?;
        runtime
            .last_turn
            .and_then(|t| t.prompt_preview)
            .context("No turn snapshot available for prompt preview")
    }

    pub async fn reload_session_system_prompt(&self, session_id: &str) -> Result<SessionSystemPromptReloadResponse> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let agent_id = {
            let control = session.control.read().unwrap();
            control.active_agent.clone()
        };
        let agent_descriptor = self
            .agent_registry
            .get(&agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' not found", agent_id))?;

        let app_config_snapshot = self
            .config
            .read()
            .map_err(|_| anyhow::anyhow!("Application config lock poisoned"))?
            .clone();
        let reloaded_origin = OriginAppConfig::load_from_file(app_config_snapshot.config_path())?;
        let reloaded_config = AppConfig::from_origin(reloaded_origin, app_config_snapshot.config_dir.clone());

        let agent_spec = reloaded_config
            .gateway
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' missing in config", agent_id))?;
        let prompt_base = load_agent_prompt_for_reload(&agent_spec, &reloaded_config).await?;
        let prompt_base_fingerprint = fingerprint_text(&prompt_base);

        let mut prompt_config = PromptConfig::new(agent_id.clone(), prompt_base.clone(), None)
            .with_project_context_path_opt(reloaded_config.project_context_file())
            .with_workflow_prompt_path(reloaded_config.prompts_dir().join("workflow-stages.md"))
            .with_template_vars(agent_descriptor.initial_template_vars.clone());
        let env = crate::prompt::EnvironmentSnapshot::collect(&reloaded_config.config_dir, None).await;
        prompt_config = prompt_config.with_environment(env);
        let compiled_prompt = SystemPromptBuilder::from_config(&prompt_config, &default_skill_registry()).build();
        let prompt_version = fingerprint_text(&compiled_prompt);
        let source_revision = source_revision(&reloaded_config).await;
        log::info!(
            "Session prompt reload prepared: session_id={}, agent_id={}, source_revision={}, prompt_base_len={}, prompt_base_hash={}, compiled_prompt_len={}, compiled_prompt_hash={}",
            session_id,
            agent_id,
            source_revision,
            prompt_base.len(),
            prompt_base_fingerprint,
            compiled_prompt.len(),
            prompt_version
        );

        let (version_before, version_after, changed, updated_at) = self
            .sessions
            .reload_system_prompt(session_id, prompt_base, prompt_version, source_revision)
            .await?;
        log::info!(
            "Session system prompt reloaded: session_id={}, version_before={}, version_after={}, changed={}",
            session_id,
            version_before,
            version_after,
            changed
        );

        Ok(SessionSystemPromptReloadResponse {
            session_id: session_id.to_string(),
            version_before,
            version_after,
            updated_at,
            changed,
        })
    }

    pub async fn list_session_tools(&self, session_id: &str) -> Result<SessionToolsResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionToolsResponse {
            tools: runtime.last_turn.map(|t| t.tools).unwrap_or_default(),
            updated_at: runtime.updated_at,
        })
    }

    pub async fn list_session_file_tree(
        &self,
        session_id: &str,
        relative_path: Option<String>,
    ) -> Result<SessionFileTreeResponse> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let project_dir = {
            let control = session.control.read().unwrap();
            control
                .project_dir
                .clone()
                .context("Session project directory is not set")?
        };

        let base_relative_path = normalize_relative_path(relative_path.as_deref());
        let target_path = resolve_directory_target(&project_dir, &base_relative_path)?;
        let mut entries = read_dir_entries(&project_dir, &target_path, &base_relative_path).await?;
        sort_file_tree_entries(&mut entries);

        Ok(SessionFileTreeResponse {
            entries,
            base_relative_path,
            project_dir_present: true,
            updated_at: Utc::now().timestamp_millis(),
        })
    }

    pub async fn list_session_skill_bindings(&self, session_id: &str) -> Result<SessionSkillBindingsResponse> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().unwrap();
        let skills = deserialize_skill_bindings(&control.skill_bindings);
        let resp = SessionSkillBindingsResponse {
            skills,
            updated_at: Utc::now().timestamp_millis(),
        };
        if let Ok(json) = serde_json::to_string(&resp) {
            log::info!("[SKILL_REC] Final Serialization to Frontend: {}", json);
        }
        Ok(resp)
    }

    pub async fn get_session_memory_hits(
        &self,
        session_id: &str,
        _turn_id: Option<String>,
    ) -> Result<SessionMemoryHitsResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionMemoryHitsResponse {
            hits: runtime.last_turn.map(|t| t.memory_hits).unwrap_or_default(),
            enabled: true,
            updated_at: runtime.updated_at,
        })
    }

    pub async fn override_session_model(
        &self,
        session_id: &str,
        req: SessionModelOverrideRequest,
    ) -> Result<SessionRuntimeSnapshot> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let active_agent = {
            let control = session.control.read().unwrap();
            control.active_agent.clone()
        };
        let config = self
            .config
            .read()
            .map_err(|_| anyhow::anyhow!("Application config lock poisoned"))?
            .clone();
        let base_binding = config.resolve_agent_binding_by_id(active_agent.as_str())?;
        let orchestration = req
            .orchestration
            .map(|m| {
                config
                    .resolve_model_override(&base_binding, m.provider.as_str(), m.model.as_str())
                    .map(|binding| ModelRef {
                        provider: binding.provider_id,
                        model: binding.model_config.model,
                    })
            })
            .transpose()?;
        let execution = req
            .execution
            .map(|m| {
                config
                    .resolve_model_override(&base_binding, m.provider.as_str(), m.model.as_str())
                    .map(|binding| ModelRef {
                        provider: binding.provider_id,
                        model: binding.model_config.model,
                    })
            })
            .transpose()?;
        let session = self
            .sessions
            .override_model(session_id, orchestration, execution)
            .await?;

        let control = session.control.read().unwrap();
        Ok(RuntimeSnapshotAssembler::assemble_session_runtime(session_id, &control))
    }

    pub async fn get_session_token_usage(&self, session_id: &str) -> Result<SessionTokenUsageResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        let repo = self.sessions.get_repository();
        let quality = repo.count_usage_quality(session_id).await?;
        let runs = repo.list_runs(session_id).await?;
        let last_turn_usage = runs
            .into_iter()
            .find_map(|run| run.usage.as_ref().and_then(map_turn_usage));
        Ok(SessionTokenUsageResponse {
            summary: SessionTokenUsageSummary {
                input_tokens: runtime.token_counters.input_tokens,
                output_tokens: runtime.token_counters.output_tokens,
                cache_creation_input_tokens: runtime.token_counters.cache_creation_input_tokens,
                cache_read_input_tokens: runtime.token_counters.cache_read_input_tokens,
                total_turn_count: quality.total_turns,
                turns_with_unknown_cache_usage: quality.turns_with_unknown_cache,
                turns_with_missing_usage: quality.turns_with_missing_usage,
                last_turn_usage,
                updated_at: runtime.updated_at,
            },
        })
    }

    pub async fn get_session_token_usage_detail(
        &self,
        session_id: &str,
        limit: u32,
        before_turn_id: Option<&str>,
    ) -> Result<SessionTokenUsageDetailResponse> {
        let repo = self.sessions.get_repository();
        let runs = repo.list_runs(session_id).await?;
        let mut details = runs
            .into_iter()
            .map(|run| TurnUsageDetail {
                turn_id: run.id.clone(),
                run_id: run.id,
                status: run.status,
                model: run.execution_model.as_ref().map(|model| model.model.clone()),
                provider: run.execution_model.as_ref().map(|model| model.provider.clone()),
                usage: run.usage.as_ref().and_then(map_turn_usage),
                started_at: run.created_at,
                finished_at: Some(run.updated_at),
            })
            .collect::<Vec<_>>();

        if let Some(before) = before_turn_id {
            if let Some(position) = details.iter().position(|item| item.turn_id == before) {
                details = details.into_iter().skip(position + 1).collect();
            }
        }

        let has_more = details.len() > limit as usize;
        let turns = details.into_iter().take(limit as usize).collect();
        Ok(SessionTokenUsageDetailResponse {
            session_id: session_id.to_string(),
            turns,
            has_more,
        })
    }

    // --- Plan 2: Execution Records & Control ---

    pub async fn list_session_runs(&self, session_id: &str) -> Result<SessionRunsResponse> {
        let repo = self.sessions.get_repository();
        let runs = repo.list_runs(session_id).await?;

        let mut proto_runs = Vec::new();
        for r in runs {
            proto_runs.push(nova_protocol::observability::RunRecord {
                run_id: r.id,
                session_id: r.session_id,
                turn_id: "".to_string(),
                agent_id: "".to_string(),
                status: r.status,
                started_at: r.created_at,
                finished_at: Some(r.updated_at),
                duration_ms: Some((r.updated_at - r.created_at) as u64),
                orchestration_model: r.orchestration_model.as_ref().map(proto_model_ref),
                execution_model: r.execution_model.as_ref().map(proto_model_ref),
                tool_call_count: r.tool_call_count,
                usage: r.usage.as_ref().and_then(map_turn_usage),
                error_summary: None,
                waiting_reason: None,
            });
        }

        Ok(SessionRunsResponse { runs: proto_runs })
    }

    pub async fn get_run_detail(&self, run_id: &str) -> Result<nova_protocol::observability::RunRecord> {
        let repo = self.sessions.get_repository();
        let r = repo.get_run(run_id).await?.context("Run not found")?;

        Ok(nova_protocol::observability::RunRecord {
            run_id: r.id.clone(),
            session_id: r.session_id,
            turn_id: r.id,
            agent_id: "".to_string(),
            status: r.status,
            started_at: r.created_at,
            finished_at: Some(r.updated_at),
            duration_ms: Some((r.updated_at - r.created_at) as u64),
            orchestration_model: r.orchestration_model.as_ref().map(proto_model_ref),
            execution_model: r.execution_model.as_ref().map(proto_model_ref),
            tool_call_count: r.tool_call_count,
            usage: r.usage.as_ref().and_then(map_turn_usage),
            error_summary: None,
            waiting_reason: None,
        })
    }

    pub async fn control_run(&self, run_id: &str, req: RunControlRequest) -> Result<()> {
        match req.action.as_str() {
            "stop" => {
                // Update DB status
                let repo = self.sessions.get_repository();
                repo.update_run_status(run_id, "stopped", Utc::now().timestamp_millis())
                    .await?;

                // Try to find and cancel the associated session's cancellation token.
                // The run_id is also the turn_id in our implementation, and the session_id
                // can be looked up from the run record.
                if let Ok(Some(run)) = repo.get_run(run_id).await {
                    if let Ok(Some(session)) = self.sessions.get(&run.session_id).await {
                        if let Some(token) = session.take_cancellation_token() {
                            token.cancel();
                        }
                    }
                }
            }
            "pause" | "resume" | "retry" => {
                anyhow::bail!("capability_not_supported: {} is not yet implemented", req.action);
            }
            _ => {
                let repo = self.sessions.get_repository();
                repo.update_run_status(run_id, &req.action, Utc::now().timestamp_millis())
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn list_session_artifacts(&self, session_id: &str) -> Result<SessionArtifactsResponse> {
        let repo = self.sessions.get_repository();
        let artifacts = repo.list_artifacts(session_id).await?;

        let mut proto_artifacts = Vec::new();
        for a in artifacts {
            proto_artifacts.push(nova_protocol::observability::ArtifactRecord {
                artifact_id: a.id,
                session_id: a.session_id,
                run_id: a.run_id.unwrap_or_default(),
                step_id: "".to_string(),
                artifact_type: a.content_type,
                path: a.storage_path,
                filename: a.name,
                content_preview: None,
                language: None,
                size: 0,
                created_at: a.created_at,
            });
        }

        Ok(SessionArtifactsResponse {
            artifacts: proto_artifacts,
        })
    }

    pub async fn list_pending_permissions(&self, session_id: Option<&str>) -> Result<PermissionPendingResponse> {
        let repo = self.sessions.get_repository();
        let session_id_str = match session_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                // No session_id provided or empty - return empty list
                return Ok(PermissionPendingResponse { requests: Vec::new() });
            }
        };
        let requests = repo.list_permission_requests(session_id_str).await?;

        let mut proto_requests = Vec::new();
        for r in requests {
            proto_requests.push(nova_protocol::observability::PermissionRequestRecord {
                request_id: r.id,
                session_id: r.session_id,
                run_id: r.run_id,
                step_id: "".to_string(),
                agent_id: "".to_string(),
                kind: r.capability,
                title: r.resource.clone(),
                reason: r.reason,
                target: r.resource,
                risk_level: "unknown".to_string(),
                status: r.status,
                created_at: r.created_at,
                resolved_at: None,
            });
        }

        Ok(PermissionPendingResponse {
            requests: proto_requests,
        })
    }

    pub async fn respond_to_permission(&self, req: PermissionRespondRequest) -> Result<()> {
        let repo = self.sessions.get_repository();
        repo.resolve_permission_request(&req.request_id, &req.action, None)
            .await?;
        Ok(())
    }

    pub async fn list_audit_logs(&self, session_id: &str) -> Result<AuditLogsResponse> {
        let repo = self.sessions.get_repository();
        let logs = repo.list_audit_logs(session_id).await?;

        let mut proto_logs = Vec::new();
        for l in logs {
            proto_logs.push(nova_protocol::observability::AuditLogRecord {
                log_id: l.id.to_string(),
                session_id: l.session_id,
                run_id: l.run_id,
                action: l.action,
                actor: "system".to_string(),
                detail: serde_json::to_string(&l.details).unwrap_or_default(),
                created_at: l.created_at,
            });
        }

        Ok(AuditLogsResponse { logs: proto_logs })
    }

    pub async fn get_diagnostics(&self, session_id: &str) -> Result<DiagnosticsResponse> {
        let repo = self.sessions.get_repository();
        let issues = repo.list_diagnostics(session_id).await?;

        let mut proto_issues = Vec::new();
        for i in issues {
            proto_issues.push(nova_protocol::observability::DiagnosticIssueRecord {
                issue_id: i.id,
                category: "unknown".to_string(),
                title: i.message.clone(),
                severity: i.severity,
                message: i.message,
                action_hint: i.details.map(|v| serde_json::to_string(&v).unwrap_or_default()),
                count: 1,
                created_at: i.created_at,
                updated_at: i.created_at,
            });
        }

        Ok(DiagnosticsResponse { issues: proto_issues })
    }

    pub async fn restore_workspace(&self) -> Result<WorkspaceRestoreResponse> {
        let repo = self.sessions.get_repository();
        let state = repo.get_last_workspace_restore_state().await?;

        match state {
            Some(state) => {
                let snapshot = state.snapshot;
                Ok(WorkspaceRestoreResponse {
                    session_id: snapshot.get("session_id").and_then(|v| v.as_str()).map(String::from),
                    agent_id: snapshot.get("agent_id").and_then(|v| v.as_str()).map(String::from),
                    console_visible: snapshot
                        .get("console_visible")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    active_tab: snapshot
                        .get("active_tab")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| "chat".to_string()),
                    selected_run_id: snapshot
                        .get("selected_run_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    selected_artifact_id: snapshot
                        .get("selected_artifact_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    selected_permission_request_id: snapshot
                        .get("selected_permission_request_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    selected_diagnostic_id: snapshot
                        .get("selected_diagnostic_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    restorable_run_state: snapshot
                        .get("restorable_run_state")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| "none".to_string()),
                    updated_at: state.updated_at,
                })
            }
            None => {
                // No restore state found - return empty default (design principle #4: distinguish "no data" from "error")
                Ok(WorkspaceRestoreResponse {
                    session_id: None,
                    agent_id: None,
                    console_visible: false,
                    active_tab: "chat".to_string(),
                    selected_run_id: None,
                    selected_artifact_id: None,
                    selected_permission_request_id: None,
                    selected_diagnostic_id: None,
                    restorable_run_state: "none".to_string(),
                    updated_at: 0,
                })
            }
        }
    }
}

fn default_skill_registry() -> crate::skill::SkillRegistry {
    crate::skill::SkillRegistry::new()
}

async fn load_agent_prompt_for_reload(agent: &crate::config::AgentSpec, config: &AppConfig) -> Result<String> {
    if agent.prompt_file.is_some() && agent.prompt_inline.is_some() {
        anyhow::bail!(
            "Agent '{}' has both prompt_file and prompt_inline configured; only one is allowed",
            agent.id
        );
    }
    if let Some(file) = &agent.prompt_file {
        let prompt_path = config.prompts_dir().join(file);
        return fs::read_to_string(&prompt_path)
            .await
            .with_context(|| format!("Failed to read prompt_file for agent '{}': {:?}", agent.id, prompt_path));
    }
    if let Some(inline) = &agent.prompt_inline {
        return Ok(inline.clone());
    }
    let prompt_file = format!("agent-{}.md", agent.id);
    let prompt_path = config.prompts_dir().join(&prompt_file);
    match fs::read_to_string(&prompt_path).await {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()),
    }
}

fn fingerprint_text(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

async fn source_revision(config: &AppConfig) -> String {
    let path = config.config_path();
    match fs::metadata(&path).await {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            format!("mtime:{}:len:{}", modified, meta.len())
        }
        Err(_) => "unknown".to_string(),
    }
}

fn proto_model_ref(model: &ModelRef) -> nova_protocol::ModelRef {
    nova_protocol::ModelRef {
        provider: model.provider.clone(),
        model: model.model.clone(),
    }
}

fn map_turn_usage(value: &serde_json::Value) -> Option<TurnUsage> {
    serde_json::from_value::<TurnUsage>(value.clone()).ok()
}

fn deserialize_skill_bindings(
    bindings: &[serde_json::Value],
) -> Vec<nova_protocol::observability::SkillBindingSnapshot> {
    bindings
        .iter()
        .filter_map(|value| {
            let skill_id = value
                .get("skill_id")
                .or_else(|| value.get("skillId"))
                .and_then(|item| item.as_str())?;
            let name = value.get("name").and_then(|item| item.as_str())?;
            let status = value.get("status").and_then(|item| item.as_str())?;
            let description = value
                .get("description")
                .and_then(|item| item.as_str())
                .map(ToString::to_string);
            Some(nova_protocol::observability::SkillBindingSnapshot {
                skill_id: skill_id.to_string(),
                name: name.to_string(),
                status: status.to_string(),
                description,
            })
        })
        .collect()
}

fn normalize_relative_path(raw: Option<&str>) -> String {
    let trimmed = raw.unwrap_or("").trim().trim_start_matches('@').trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.replace('\\', "/")
    }
}

fn resolve_directory_target(project_dir: &Path, base_relative_path: &str) -> Result<PathBuf> {
    let lookup = if base_relative_path.is_empty() {
        ".".to_string()
    } else {
        base_relative_path.to_string()
    };
    let resolved = resolve_path_ref(&lookup, project_dir, Some(project_dir), true)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !resolved.is_dir {
        anyhow::bail!("Target path is not a directory: {}", resolved.target_path.display());
    }
    Ok(resolved.target_path)
}

async fn read_dir_entries(
    project_dir: &Path,
    target_path: &Path,
    base_relative_path: &str,
) -> Result<Vec<SessionFileTreeEntry>> {
    let mut reader = fs::read_dir(target_path)
        .await
        .with_context(|| format!("Failed to read directory: {}", target_path.display()))?;
    let mut entries = Vec::new();

    while let Some(item) = reader.next_entry().await? {
        let name = item.file_name().to_string_lossy().to_string();
        let file_type = item.file_type().await?;
        let abs = item.path();
        let rel = abs
            .strip_prefix(project_dir)
            .with_context(|| format!("Path is out of project root: {}", abs.display()))?;
        entries.push(SessionFileTreeEntry {
            name,
            relative_path: rel.to_string_lossy().replace('\\', "/"),
            is_dir: file_type.is_dir(),
        });
    }

    if !base_relative_path.is_empty() && entries.is_empty() {
        return Ok(Vec::new());
    }

    Ok(entries)
}

fn sort_file_tree_entries(entries: &mut [SessionFileTreeEntry]) {
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
}

#[cfg(test)]
mod tests {
    use super::{deserialize_skill_bindings, sort_file_tree_entries};
    use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
    use crate::app::agent_workspace_service::AgentWorkspaceService;
    use crate::config::{AppConfig, OriginAppConfig};
    use crate::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
    use nova_protocol::observability::SessionFileTreeEntry;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};
    use tempfile::tempdir;

    #[test]
    fn session_skill_bindings_reading_does_not_depend_on_last_turn() {
        let bindings = vec![serde_json::json!({
            "skill_id":"skill-a",
            "name":"Skill A",
            "status":"active",
            "description": serde_json::Value::Null
        })];

        let snapshots = deserialize_skill_bindings(&bindings);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].skill_id, "skill-a");
    }

    #[test]
    fn file_tree_entries_are_sorted_by_dir_then_name_case_insensitive() {
        let mut entries = vec![
            SessionFileTreeEntry {
                name: "zeta.rs".to_string(),
                relative_path: "zeta.rs".to_string(),
                is_dir: false,
            },
            SessionFileTreeEntry {
                name: "Beta".to_string(),
                relative_path: "Beta".to_string(),
                is_dir: true,
            },
            SessionFileTreeEntry {
                name: "alpha".to_string(),
                relative_path: "alpha".to_string(),
                is_dir: true,
            },
            SessionFileTreeEntry {
                name: "Alpha.txt".to_string(),
                relative_path: "Alpha.txt".to_string(),
                is_dir: false,
            },
        ];

        sort_file_tree_entries(&mut entries);
        let names = entries.into_iter().map(|item| item.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "Beta", "Alpha.txt", "zeta.rs"]);
    }

    #[tokio::test]
    async fn inspect_agent_returns_real_provider_id() {
        let dir = tempdir().expect("temp dir should exist");
        let manager = SqliteManager::new(dir.path()).await.expect("sqlite should init");
        let repository = SqliteSessionRepository::new(manager.pool.clone());
        let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = sessions
            .create(Some("s".to_string()), "developer".to_string(), String::new())
            .await
            .expect("session should create");

        let mut origin = OriginAppConfig::default();
        origin.providers.insert(
            "cloud".to_string(),
            crate::config::ProviderConfig {
                api_key: String::new(),
                base_url: "https://api.openai.com/v1".to_string(),
            },
        );
        origin.llms.insert(
            "cloud_gpt4o".to_string(),
            crate::config::RegisteredLlmConfig {
                provider: "cloud".to_string(),
                model_config: crate::provider::ModelConfig {
                    provider: Some("cloud".to_string()),
                    model: "gpt-4o".to_string(),
                    max_tokens: 4096,
                    temperature: Some(0.3),
                    top_p: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
            },
        );
        origin.defaults.provider = "default".to_string();
        origin.defaults.llm = "default".to_string();
        origin.gateway.agents = vec![crate::config::AgentSpec {
            id: "developer".to_string(),
            display_name: "Developer".to_string(),
            description: "dev".to_string(),
            aliases: Vec::new(),
            provider: "cloud".to_string(),
            llm: Some("cloud_gpt4o".to_string()),
            prompt_file: None,
            prompt_inline: None,
            system_prompt_template: None,
            tool_whitelist: None,
            model_config: None,
        }];
        let config = Arc::new(RwLock::new(AppConfig::from_origin(origin, PathBuf::from("."))));
        let registry = AgentRegistry::new(AgentDescriptor {
            id: "developer".to_string(),
            display_name: "Developer".to_string(),
            description: "dev".to_string(),
            aliases: Vec::new(),
            system_prompt_template: String::new(),
            system_prompt_base: String::new(),
            initial_template_vars: HashMap::new(),
            tool_whitelist: None,
            model_config: None,
            provider_id: "cloud".to_string(),
            llm_id: Some("cloud_gpt4o".to_string()),
        });
        let service = AgentWorkspaceService::new(registry, sessions, config);

        let response = service
            .inspect_agent("developer", &session.id)
            .await
            .expect("inspect should succeed");
        assert_eq!(response.effective_model.orchestration.provider, "cloud");
        assert_eq!(response.effective_model.execution.provider, "cloud");
    }
}
