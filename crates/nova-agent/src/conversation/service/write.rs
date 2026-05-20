use super::helpers::{normalize_project_dir, sync_last_turn_prompt_preview};
use super::{SessionService, DEFAULT_SESSION_TITLE};
use crate::conversation::control::{ControlState, LastTurnSnapshot, ModelRef, TitleState};
use crate::conversation::session::Session;
use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

impl SessionService {
    /// 创建一个新会话并持久化
    pub async fn create(&self, name: Option<String>, agent_id: String, system_prompt: String) -> Result<Arc<Session>> {
        self.create_for_agent(name, agent_id, system_prompt, None).await
    }

    pub async fn create_for_agent(
        &self,
        name: Option<String>,
        agent_id: String,
        system_prompt: String,
        inherited_project_dir: Option<PathBuf>,
    ) -> Result<Arc<Session>> {
        let id = Uuid::new_v4().to_string();
        let session_name = name.unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string());
        let now = Utc::now().timestamp_millis();

        let mut initial_history = Vec::new();
        if !system_prompt.is_empty() {
            initial_history.push(Message {
                id: Uuid::new_v4().to_string(),
                role: Role::System,
                content: vec![ContentBlock::Text { text: system_prompt }],
                created_at: now,
                metadata: None,
            });
        }

        let session = Arc::new(Session {
            control: tokio::sync::RwLock::new(ControlState::new_with_project_dir(&agent_id, inherited_project_dir)),
            id: id.clone(),
            name: RwLock::new(session_name),
            history: RwLock::new(initial_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
            title_state: RwLock::new(TitleState::new_default()),
            parent_session_id: None,
            parent_tool_use_id: None,
            child_session_ids: RwLock::new(Vec::new()),
        });

        self.persist_full_session(&session).await?;
        self.cache.insert_loaded(id, session.clone()).await;
        Ok(session)
    }

    pub async fn append_message(
        &self,
        session_id: &str,
        role: Role,
        content: Vec<ContentBlock>,
        metadata: Option<Value>,
    ) -> Result<()> {
        let session = self
            .ensure_session_history_loaded(session_id)
            .await?
            .context("Session not found")?;
        let now = Utc::now().timestamp_millis();
        let message_id = Uuid::new_v4().to_string();
        let mut parsed_metadata = metadata
            .clone()
            .map(serde_json::from_value::<crate::message::MessageMetadata>)
            .transpose()?;
        if let Some(metadata) = parsed_metadata.as_mut() {
            if let Some(trace) = metadata.provider_http_trace.as_mut() {
                trace.bound_message_id = message_id.clone();
            }
        }
        let persisted_metadata = parsed_metadata.as_ref().map(serde_json::to_value).transpose()?;

        {
            let mut history = session.history.write().await;
            history.push(Message {
                id: message_id.clone(),
                role: role.clone(),
                content: content.clone(),
                created_at: now,
                metadata: parsed_metadata,
            });
            session.touch_updated_at();
        }

        let is_user = role == Role::User;
        self.repository
            .save_message(session_id, &message_id, role, content, persisted_metadata, now)
            .await?;
        self.persist_session_control(&session).await?;
        if is_user {
            self.maybe_schedule_title_generation(session).await?;
        }

        Ok(())
    }

    pub async fn touch_session(&self, session_id: &str) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let updated_at = Utc::now().timestamp_millis();
        session.updated_at.store(updated_at, Ordering::SeqCst);
        self.repository.touch_session(session_id, updated_at).await?;
        Ok(session)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.repository.delete_session(id).await?;
        Ok(self.cache.remove(id).await.is_some())
    }

    pub async fn copy_session(&self, source_id: &str, truncate_index: Option<usize>) -> Result<Option<Arc<Session>>> {
        let source = self
            .ensure_session_history_loaded(source_id)
            .await?
            .context("Source session not found")?;

        let history = source.get_history().await;
        let new_history = if let Some(idx) = truncate_index {
            if idx < history.len() {
                history[..=idx].to_vec()
            } else {
                history
            }
        } else {
            history
        };

        let new_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let new_control = {
            let source_control = source.control.read().await;
            let agent_id = source_control.active_agent.clone();
            let mut new_control = ControlState::new_with_project_dir(&agent_id, source_control.project_dir.clone());
            new_control.model_override = source_control.model_override.clone();
            new_control
        };

        // copy_session 副本视为独立根 Session，不继承父子关系
        // （见 docs/2026-05-20-session-parent-child-tree 总览「已收敛的待澄清点」#1）。
        let session = Arc::new(Session {
            control: tokio::sync::RwLock::new(new_control),
            id: new_id.clone(),
            name: RwLock::new(format!("{} (Copy)", source.get_name().await)),
            history: RwLock::new(new_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
            title_state: RwLock::new(TitleState::new_default()),
            parent_session_id: None,
            parent_tool_use_id: None,
            child_session_ids: RwLock::new(Vec::new()),
        });

        self.persist_full_session(&session).await?;
        self.cache.insert_loaded(new_id, session.clone()).await;
        Ok(Some(session))
    }

    pub async fn override_model(
        &self,
        session_id: &str,
        orchestration: Option<ModelRef>,
        execution: Option<ModelRef>,
    ) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().await;
            control.model_override.orchestration = orchestration;
            control.model_override.execution = execution;
            control.model_override.updated_at = Utc::now().timestamp_millis();
        }

        self.persist_runtime_control(session_id, &session).await?;
        Ok(session)
    }

    pub async fn reload_system_prompt(
        &self,
        session_id: &str,
        prompt_base_override: String,
        prompt_version: String,
        source_revision: String,
    ) -> Result<(String, String, bool, i64)> {
        let session = self
            .ensure_session_history_loaded(session_id)
            .await?
            .context("Session not found")?;
        let updated_at = Utc::now().timestamp_millis();
        let (version_before, changed, prompt_preview_synced) = {
            let mut control = session.control.write().await;
            let before = control.system_prompt_state.version.clone();
            let changed = before != prompt_version;
            control.system_prompt_base_override = Some(prompt_base_override.clone());
            control.system_prompt_state.version = prompt_version.clone();
            control.system_prompt_state.updated_at = updated_at;
            control.system_prompt_state.source_revision = source_revision;
            let prompt_preview_synced =
                sync_last_turn_prompt_preview(control.last_turn_snapshot.as_mut(), &prompt_base_override);
            (before, changed, prompt_preview_synced)
        };

        {
            let mut history = session.history.write().await;
            if let Some(first) = history.first_mut() {
                if first.role == Role::System {
                    first.content = vec![ContentBlock::Text {
                        text: prompt_base_override,
                    }];
                }
            }
        }

        self.persist_runtime_control(session_id, &session).await?;
        log::info!(
            "Session prompt override persisted: session_id={}, changed={}, synced_last_turn_prompt_preview={}",
            session_id,
            changed,
            prompt_preview_synced
        );
        Ok((version_before, prompt_version, changed, updated_at))
    }

    pub async fn update_runtime_state(
        &self,
        session_id: &str,
        snapshot: Option<LastTurnSnapshot>,
        token_delta: Option<(u64, u64, u64, u64)>,
        new_skills: Option<Vec<serde_json::Value>>,
    ) -> Result<()> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().await;
            if let Some(snapshot) = snapshot {
                control.last_turn_snapshot = Some(snapshot);
            }
            if let Some((input, output, cache_creation, cache_read)) = token_delta {
                control.token_counters.input_tokens += input;
                control.token_counters.output_tokens += output;
                control.token_counters.cache_creation_input_tokens += cache_creation;
                control.token_counters.cache_read_input_tokens += cache_read;
                control.token_counters.updated_at = Utc::now().timestamp_millis();
            }
            if let Some(skills) = new_skills {
                let prev_len = control.skill_bindings.len();
                super::skill_bindings::merge_skill_bindings(&mut control.skill_bindings, skills);
                log::info!(
                    "[SKILL_REC] DB Update: session_id={}, prev_count={}, new_count={}, incoming_updates={}",
                    session_id,
                    prev_len,
                    control.skill_bindings.len(),
                    control.skill_bindings.len().saturating_sub(prev_len)
                );
            }
        }

        self.persist_runtime_control(session_id, &session).await?;
        Ok(())
    }

    pub async fn set_project_dir(&self, session_id: &str, project_dir: &Path) -> Result<PathBuf> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let normalized = normalize_project_dir(project_dir).await;

        {
            let mut control = session.control.write().await;
            control.project_dir = Some(normalized.clone());
        }

        self.persist_runtime_control(session_id, &session).await?;
        log::info!(
            "Session project_dir updated: session_id={}, project_dir={}",
            session_id,
            normalized.display()
        );

        Ok(normalized)
    }
}
