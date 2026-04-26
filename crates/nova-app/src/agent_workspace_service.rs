use crate::snapshot_assembler::RuntimeSnapshotAssembler;
use anyhow::{Context, Result};
use chrono::Utc;
use nova_conversation::SessionService;
use nova_core::agent_catalog::AgentRegistry;
use nova_protocol::observability::*;

pub struct AgentWorkspaceService {
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
}

impl AgentWorkspaceService {
    pub fn new(agent_registry: AgentRegistry, sessions: SessionService) -> Self {
        Self {
            agent_registry,
            sessions,
        }
    }

    pub async fn inspect_agent(&self, agent_id: &str, session_id: &str) -> Result<AgentInspectResponse> {
        let session = self.sessions.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().unwrap();

        // In a real implementation, we would resolve the effective model based on:
        // 1. session override
        // 2. agent default
        // 3. global default

        let orchestration = control
            .model_override
            .orchestration
            .as_ref()
            .map(|m| nova_protocol::ModelRef {
                provider: m.provider.clone(),
                model: m.model.clone(),
            })
            .unwrap_or(nova_protocol::ModelRef {
                provider: "openai".to_string(), // Placeholder
                model: "gpt-4o".to_string(),
            });

        let execution = control
            .model_override
            .execution
            .as_ref()
            .map(|m| nova_protocol::ModelRef {
                provider: m.provider.clone(),
                model: m.model.clone(),
            })
            .unwrap_or(nova_protocol::ModelRef {
                provider: "openai".to_string(), // Placeholder
                model: "gpt-4o".to_string(),
            });

        Ok(AgentInspectResponse {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            effective_model: ModelBindingDetailView {
                orchestration,
                execution,
                source: if control.model_override.orchestration.is_some() || control.model_override.execution.is_some()
                {
                    "session_override".to_string()
                } else {
                    "agent_default".to_string()
                },
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

    pub async fn list_session_tools(&self, session_id: &str) -> Result<SessionToolsResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionToolsResponse {
            tools: runtime.last_turn.map(|t| t.tools).unwrap_or_default(),
            updated_at: runtime.updated_at,
        })
    }

    pub async fn list_session_skill_bindings(&self, session_id: &str) -> Result<SessionSkillBindingsResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionSkillBindingsResponse {
            skills: runtime.last_turn.map(|t| t.skills).unwrap_or_default(),
            updated_at: runtime.updated_at,
        })
    }

    pub async fn get_session_memory_hits(
        &self,
        session_id: &str,
        _turn_id: Option<String>,
    ) -> Result<SessionMemoryHitsResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionMemoryHitsResponse {
            hits: runtime.last_turn.map(|t| t.memory_hits).unwrap_or_default(),
            enabled: true, // Placeholder
            updated_at: runtime.updated_at,
        })
    }

    pub async fn override_session_model(
        &self,
        session_id: &str,
        req: SessionModelOverrideRequest,
    ) -> Result<SessionRuntimeSnapshot> {
        let session = self
            .sessions
            .override_model(
                session_id,
                req.orchestration.map(|m| nova_conversation::control::ModelRef {
                    provider: m.provider,
                    model: m.model,
                }),
                req.execution.map(|m| nova_conversation::control::ModelRef {
                    provider: m.provider,
                    model: m.model,
                }),
            )
            .await?;

        let control = session.control.read().unwrap();
        Ok(RuntimeSnapshotAssembler::assemble_session_runtime(session_id, &control))
    }

    pub async fn get_session_token_usage(&self, session_id: &str) -> Result<SessionTokenUsageResponse> {
        let runtime = self.get_session_runtime(session_id).await?;
        Ok(SessionTokenUsageResponse {
            usage: runtime.token_counters,
            updated_at: runtime.updated_at,
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
                turn_id: "".to_string(), // In current implementation, turn_id is run_id
                agent_id: "".to_string(), // We don't save agent_id in run table right now
                status: r.status,
                started_at: r.created_at,
                finished_at: Some(r.updated_at),
                duration_ms: Some((r.updated_at - r.created_at) as u64),
                orchestration_model: None,
                execution_model: None,
                usage: None,
                error_summary: None,
                waiting_reason: None,
            });
        }
        
        Ok(SessionRunsResponse {
            runs: proto_runs,
        })
    }

    pub async fn get_run_detail(&self, run_id: &str) -> Result<nova_protocol::observability::RunRecord> {
        let repo = self.sessions.get_repository();
        let r = repo.get_run(run_id).await?.context("Run not found")?;
        
        Ok(nova_protocol::observability::RunRecord {
            run_id: r.id.clone(),
            session_id: r.session_id,
            turn_id: r.id, // Using run_id as turn_id for now
            agent_id: "".to_string(),
            status: r.status,
            started_at: r.created_at,
            finished_at: Some(r.updated_at),
            duration_ms: Some((r.updated_at - r.created_at) as u64),
            orchestration_model: None,
            execution_model: None,
            usage: None,
            error_summary: None,
            waiting_reason: None,
        })
    }

    pub async fn control_run(&self, run_id: &str, req: RunControlRequest) -> Result<()> {
        let repo = self.sessions.get_repository();
        repo.update_run_status(run_id, &req.action, Utc::now().timestamp_millis()).await?;
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
        let session_id_str = session_id.unwrap_or(""); // In real impl, we might want to query all if None
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
        repo.resolve_permission_request(&req.request_id, &req.action, None).await?;
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
        
        Ok(AuditLogsResponse {
            logs: proto_logs,
        })
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
        
        Ok(DiagnosticsResponse {
            issues: proto_issues,
        })
    }

    pub async fn restore_workspace(&self) -> Result<WorkspaceRestoreResponse> {
        let repo = self.sessions.get_repository();
        let state = repo.get_last_workspace_restore_state().await?;
        
        if let Some(state) = state {
            // Need to convert JSON value to WorkspaceRestoreState
            if let Ok(snapshot) = serde_json::from_value::<serde_json::Value>(state.snapshot) {
                Ok(WorkspaceRestoreResponse {
                    session_id: snapshot.get("session_id").and_then(|v| v.as_str()).map(String::from),
                    agent_id: snapshot.get("agent_id").and_then(|v| v.as_str()).map(String::from),
                    console_visible: snapshot.get("console_visible").and_then(|v| v.as_bool()).unwrap_or(false),
                    active_tab: snapshot.get("active_tab").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| "chat".to_string()),
                    selected_run_id: snapshot.get("selected_run_id").and_then(|v| v.as_str()).map(String::from),
                    selected_artifact_id: snapshot.get("selected_artifact_id").and_then(|v| v.as_str()).map(String::from),
                    selected_permission_request_id: snapshot.get("selected_permission_request_id").and_then(|v| v.as_str()).map(String::from),
                    selected_diagnostic_id: snapshot.get("selected_diagnostic_id").and_then(|v| v.as_str()).map(String::from),
                    restorable_run_state: snapshot.get("restorable_run_state").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| "none".to_string()),
                    updated_at: state.updated_at,
                })
            } else {
                anyhow::bail!("Invalid snapshot format in DB");
            }
        } else {
            anyhow::bail!("No restore state found for session");
        }
    }
}
