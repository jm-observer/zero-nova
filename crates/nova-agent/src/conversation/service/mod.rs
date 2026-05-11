pub mod commands;
pub mod events;
mod helpers;
pub mod queries;

use super::cache::SessionCache;
use super::control::{ControlState, TitleSource, TitleState, TitleStatus};
use super::repository::SqliteSessionRepository;
use super::session::{Session, SessionSummary};
use super::title_generator::{RuleBasedTitleGenerator, TitleGenerationError, TitleGenerator};
use crate::message::{ContentBlock, Message, Role};
use crate::tool::ProjectDirService;
use anyhow::{Context, Result};
use chrono::Utc;
use helpers::{normalize_generated_title, normalize_project_dir, sync_last_turn_prompt_preview};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

type SessionLoadResult = Option<Arc<Session>>;
type LoadingWaiters = HashMap<String, Vec<oneshot::Sender<SessionLoadResult>>>;

// 标题生成常量
/// 首次尝试触发标题生成的最小用户消息数
pub const TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT: usize = 2;
/// 第二次尝试触发标题生成的最小用户消息数
pub const TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT: usize = 3;
/// 最大尝试次数
pub const TITLE_MAX_ATTEMPTS: u8 = 2;
/// 最小总字符数（所有用户文本消息的字符总和）
pub const TITLE_MIN_TOTAL_CHARS: usize = 24;
/// 标题生成超时时间
pub const TITLE_GENERATION_TIMEOUT_MS: u64 = 3_000;

/// 默认会话标题
const DEFAULT_SESSION_TITLE: &str = "未命名会话";

#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    title_generator: Arc<dyn TitleGenerator + Send + Sync>,
    /// De-duplicates concurrent cold loads for the same session id.
    loading: Arc<RwLock<LoadingWaiters>>,
}

impl SessionService {
    pub fn new(cache: Arc<SessionCache>, repository: SqliteSessionRepository) -> Self {
        Self::new_with_title_generator(cache, repository, Arc::new(RuleBasedTitleGenerator))
    }

    pub fn new_with_title_generator(
        cache: Arc<SessionCache>,
        repository: SqliteSessionRepository,
        title_generator: Arc<dyn TitleGenerator + Send + Sync>,
    ) -> Self {
        Self {
            cache,
            repository,
            title_generator,
            loading: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_repository(&self) -> SqliteSessionRepository {
        self.repository.clone()
    }

    /// 启动阶段仅加载会话索引（不加载消息历史）。
    pub async fn load_session_index(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, title, agent_id, created_at, updated_at, runtime_control) in rows {
            let title_state = runtime_control.title_state.clone();
            let session = Arc::new(Session {
                control: tokio::sync::RwLock::new(runtime_control),
                id: id.clone(),
                name: RwLock::new(title),
                history: RwLock::new(Vec::new()),
                created_at,
                updated_at: AtomicI64::new(updated_at),
                chat_lock: Mutex::new(()),
                cancellation_token: RwLock::new(None),
                title_state: RwLock::new(title_state),
            });
            {
                let mut control = session.control.write().await;
                if control.active_agent.is_empty() {
                    control.active_agent = agent_id;
                }
            }
            self.cache.insert_indexed(id, session).await;
        }
        Ok(())
    }

    /// 从数据库加载所有会话到内存（完整 history，测试/迁移辅助）。
    pub async fn load_all(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, _title, _agent_id, _created_at, _updated_at, _runtime_control) in rows {
            if let Some(session) = self.load_session_from_db(&id).await? {
                self.cache.insert_loaded(id, session).await;
            }
        }
        Ok(())
    }

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
        });

        self.persist_full_session(&session).await?;
        self.cache.insert_loaded(id, session.clone()).await;
        Ok(session)
    }

    pub async fn find_latest_session_by_agent(&self, agent_id: &str) -> Result<Option<Arc<Session>>> {
        let Some((session_id, _title, _agent_id, _created_at, _updated_at, _runtime_control)) =
            self.repository.find_latest_session_by_agent(agent_id).await?
        else {
            return Ok(None);
        };

        self.get(&session_id).await
    }

    /// 获取会话元数据（可能未加载 history）。
    pub async fn get(&self, id: &str) -> Result<Option<Arc<Session>>> {
        if let Some(session) = self.cache.get(id).await {
            return Ok(Some(session));
        }

        let loaded = self.repository.load_session_meta(id).await?;
        let Some((id, title, agent_id, created_at, updated_at, runtime_control)) = loaded else {
            return Ok(None);
        };

        let title_state = runtime_control.title_state.clone();
        let session = Arc::new(Session {
            control: tokio::sync::RwLock::new(runtime_control),
            id: id.clone(),
            name: RwLock::new(title),
            history: RwLock::new(Vec::new()),
            created_at,
            updated_at: AtomicI64::new(updated_at),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
            title_state: RwLock::new(title_state),
        });
        {
            let mut control = session.control.write().await;
            if control.active_agent.is_empty() {
                control.active_agent = agent_id;
            }
        }
        self.cache.insert_indexed(id, session.clone()).await;
        Ok(Some(session))
    }

    /// 获取会话并确保历史消息已加载（同 session 并发去重）。
    pub async fn get_with_history(&self, id: &str) -> Result<Option<Arc<Session>>> {
        self.ensure_session_history_loaded(id).await
    }

    pub async fn ensure_session_history_loaded(&self, id: &str) -> Result<Option<Arc<Session>>> {
        if self.cache.is_history_loaded(id).await {
            return Ok(self.cache.get(id).await);
        }

        if self.cache.get(id).await.is_none() {
            if self.get(id).await?.is_none() {
                return Ok(None);
            }
            if self.cache.is_history_loaded(id).await {
                return Ok(self.cache.get(id).await);
            }
        }

        let mut receiver = None;
        let is_loader = {
            let mut loading = self.loading.write().await;
            if let Some(waiters) = loading.get_mut(id) {
                let (tx, rx) = oneshot::channel();
                waiters.push(tx);
                receiver = Some(rx);
                false
            } else {
                loading.insert(id.to_string(), Vec::new());
                true
            }
        };

        if !is_loader {
            if let Some(rx) = receiver {
                match rx.await {
                    Ok(session) => return Ok(session),
                    Err(_) => {
                        if self.cache.is_history_loaded(id).await {
                            return Ok(self.cache.get(id).await);
                        }
                    }
                }
            }
            return Ok(self.cache.get(id).await);
        }

        let load_result = self.load_session_from_db(id).await?;
        if let Some(session) = load_result.as_ref() {
            self.cache.replace_with_loaded(id.to_string(), session.clone()).await;
        }

        let waiters = {
            let mut loading = self.loading.write().await;
            loading.remove(id).unwrap_or_default()
        };
        for waiter in waiters {
            let _ = waiter.send(load_result.clone());
        }

        Ok(load_result)
    }

    async fn load_session_from_db(&self, id: &str) -> Result<Option<Arc<Session>>> {
        let loaded = self.repository.load_session(id).await?;
        Ok(loaded.map(
            |(id, title, _agent_id, created_at, updated_at, runtime_control, history)| {
                let title_state = runtime_control.title_state.clone();
                Arc::new(Session {
                    control: tokio::sync::RwLock::new(runtime_control),
                    id,
                    name: RwLock::new(title),
                    history: RwLock::new(history),
                    created_at,
                    updated_at: AtomicI64::new(updated_at),
                    chat_lock: Mutex::new(()),
                    cancellation_token: RwLock::new(None),
                    title_state: RwLock::new(title_state),
                })
            },
        ))
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

    pub async fn list_sorted(&self) -> Vec<SessionSummary> {
        let mut entries = self.cache.list_entries().await;

        entries.sort_by(|a, b| {
            b.session
                .updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.session.updated_at.load(Ordering::SeqCst))
        });

        let mut summaries = Vec::with_capacity(entries.len());
        for entry in entries {
            let session = entry.session;
            let name = session.get_name().await;
            let agent_id = session.control.read().await.active_agent.clone();
            let message_count = if entry.history_loaded {
                session.history.read().await.len()
            } else {
                0
            };
            summaries.push(SessionSummary {
                id: session.id.clone(),
                name,
                agent_id,
                created_at: session.created_at,
                updated_at: session.updated_at.load(Ordering::SeqCst),
                message_count,
            });
        }
        summaries
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
        });

        self.persist_full_session(&session).await?;
        self.cache.insert_loaded(new_id, session.clone()).await;
        Ok(Some(session))
    }

    pub async fn override_model(
        &self,
        session_id: &str,
        orchestration: Option<super::control::ModelRef>,
        execution: Option<super::control::ModelRef>,
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
        snapshot: Option<super::control::LastTurnSnapshot>,
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
                merge_skill_bindings(&mut control.skill_bindings, skills);
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

    pub async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().await;
        Ok(control.project_dir.clone())
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

    /// 持久化完整会话快照（用于 create/copy/rebuild 等路径，非常规热写入路径）。
    async fn persist_full_session(&self, session: &Arc<Session>) -> Result<()> {
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.get_name().await,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
            )
            .await?;

        for msg in session.get_history().await {
            self.repository
                .save_message(
                    &session.id,
                    &msg.id,
                    msg.role.clone(),
                    msg.content.clone(),
                    msg.metadata.as_ref().map(serde_json::to_value).transpose()?,
                    msg.created_at,
                )
                .await?;
        }

        Ok(())
    }

    async fn persist_session_control(&self, session: &Arc<Session>) -> Result<()> {
        sync_title_state_into_control(session).await;
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.get_name().await,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
            )
            .await
    }

    async fn persist_runtime_control(&self, session_id: &str, session: &Arc<Session>) -> Result<()> {
        sync_title_state_into_control(session).await;
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };
        let updated_at = Utc::now().timestamp_millis();
        session.updated_at.store(updated_at, Ordering::SeqCst);

        self.repository
            .update_session_runtime_control(session_id, &runtime_control, updated_at)
            .await
    }
}

async fn sync_title_state_into_control(session: &Arc<Session>) {
    let title_state = session.title_state.read().await.clone();
    let mut control = session.control.write().await;
    control.title_state = title_state;
}

impl SessionService {
    async fn maybe_schedule_title_generation(&self, session: Arc<Session>) -> Result<()> {
        let (can_schedule, user_messages_count, user_texts) = {
            let mut title_state = session.title_state.write().await;
            let history = session.history.read().await;

            if title_state.source != TitleSource::Default
                || title_state.status == TitleStatus::Pending
                || title_state.attempt_count >= TITLE_MAX_ATTEMPTS
            {
                return Ok(());
            }

            let user_texts: Vec<String> = history
                .iter()
                .filter(|m| m.role == Role::User)
                .flat_map(|m| m.content.iter())
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.trim()),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            let user_messages_count = user_texts.len();
            let total_chars = user_texts.iter().map(|text| text.chars().count()).sum::<usize>();
            let min_messages = if title_state.attempt_count == 0 {
                TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT
            } else {
                TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT
            };
            if user_messages_count < min_messages || total_chars < TITLE_MIN_TOTAL_CHARS {
                return Ok(());
            }

            title_state.set_pending(user_messages_count);
            (true, user_messages_count, user_texts)
        };

        if !can_schedule {
            return Ok(());
        }
        self.persist_runtime_control(&session.id, &session).await?;

        let this = self.clone();
        tokio::spawn(async move {
            if let Err(err) = this
                .run_title_generation(session.clone(), user_messages_count, user_texts)
                .await
            {
                log::error!(
                    "Session title generation task failed: session_id={}, err={}",
                    session.id,
                    err
                );
            }
        });

        Ok(())
    }

    async fn run_title_generation(
        &self,
        session: Arc<Session>,
        user_message_count: usize,
        user_texts: Vec<String>,
    ) -> Result<()> {
        let generation_result = timeout(
            Duration::from_millis(TITLE_GENERATION_TIMEOUT_MS),
            self.title_generator.generate_title(&user_texts),
        )
        .await;

        let generated = match generation_result {
            Ok(Ok(title)) => Ok(title),
            Ok(Err(TitleGenerationError::Retryable(err))) => Err(format!("retryable: {err}")),
            Ok(Err(TitleGenerationError::NonRetryable(err))) => Err(format!("non_retryable: {err}")),
            Err(_) => Err(format!("retryable: timeout after {}ms", TITLE_GENERATION_TIMEOUT_MS)),
        };

        let mut should_update_title = false;
        let mut normalized = String::new();
        {
            let mut title_state = session.title_state.write().await;
            match generated {
                Ok(title) => {
                    normalized = normalize_generated_title(&title);
                    if normalized.is_empty() {
                        title_state
                            .set_failed("non_retryable: generated title is empty after normalization".to_string());
                    } else {
                        should_update_title = true;
                    }
                }
                Err(err_msg) => title_state.set_failed(err_msg),
            }
            if should_update_title {
                title_state.set_succeeded();
                title_state.based_on_user_message_count = user_message_count;
            }
            if title_state.status == TitleStatus::Pending {
                title_state.set_failed("retryable: unexpected pending state".to_string());
            }
        }

        if should_update_title {
            session.set_name(normalized).await;
        }

        self.persist_session_control(&session).await?;
        Ok(())
    }
}

fn merge_skill_bindings(existing: &mut Vec<serde_json::Value>, incoming: Vec<serde_json::Value>) {
    let mut merged: HashMap<String, serde_json::Value> = HashMap::new();
    for skill in existing.iter() {
        if let Some(skill_id) = skill.get("skill_id").and_then(|v| v.as_str()) {
            merged.insert(skill_id.to_string(), normalize_skill_binding(skill));
        }
    }

    for skill in incoming {
        if let Some(skill_id) = skill.get("skill_id").and_then(|v| v.as_str()) {
            merged.insert(skill_id.to_string(), normalize_skill_binding(&skill));
        } else {
            log::warn!("Skipping invalid skill binding item without skill_id: {}", skill);
        }
    }

    *existing = merged.into_values().collect();
}

fn normalize_skill_binding(skill: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "skill_id": skill.get("skill_id").and_then(|v| v.as_str()).unwrap_or_default(),
        "name": skill.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
        "status": skill.get("status").and_then(|v| v.as_str()).unwrap_or_default(),
        "description": skill.get("description").cloned().unwrap_or(serde_json::Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::merge_skill_bindings;
    use super::SessionService;
    use crate::conversation::cache::SessionCache;
    use crate::conversation::control::{TitleSource, TitleState, TitleStatus};
    use crate::conversation::sqlite_manager::SqliteManager;
    use crate::conversation::title_generator::{TitleGenerationError, TitleGenerator};
    use anyhow::Result;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tempfile::tempdir;
    struct MockTitleGenerator {
        mode: MockTitleMode,
    }

    enum MockTitleMode {
        Success(String),
        RetryableError(String),
        Timeout,
    }

    #[async_trait::async_trait]
    impl TitleGenerator for MockTitleGenerator {
        async fn generate_title(&self, _user_texts: &[String]) -> Result<String, TitleGenerationError> {
            match &self.mode {
                MockTitleMode::Success(title) => Ok(title.clone()),
                MockTitleMode::RetryableError(message) => {
                    Err(TitleGenerationError::Retryable(anyhow::anyhow!(message.clone())))
                }
                MockTitleMode::Timeout => {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        super::TITLE_GENERATION_TIMEOUT_MS + 100,
                    ))
                    .await;
                    Ok("late".to_string())
                }
            }
        }
    }

    #[test]
    fn merge_skill_bindings_is_idempotent_and_deduplicates_by_skill_id() {
        let mut existing = vec![serde_json::json!({
            "skill_id": "skill-a",
            "name": "Skill A",
            "status": "active",
            "description": "v1"
        })];
        let incoming = vec![
            serde_json::json!({"skill_id":"skill-a","name":"Skill A","status":"active","description":"v1"}),
            serde_json::json!({"skill_id":"skill-a","name":"Skill A","status":"active","description":"v1"}),
        ];

        merge_skill_bindings(&mut existing, incoming);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0]["skill_id"], "skill-a");
    }

    #[test]
    fn merge_skill_bindings_overwrites_existing_fields_with_new_values() {
        let mut existing = vec![serde_json::json!({
            "skill_id": "skill-a",
            "name": "Old Name",
            "status": "inactive",
            "description": "old"
        })];
        let incoming = vec![serde_json::json!({
            "skill_id":"skill-a",
            "name":"New Name",
            "status":"active",
            "description":"new"
        })];

        merge_skill_bindings(&mut existing, incoming);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0]["name"], "New Name");
        assert_eq!(existing[0]["status"], "active");
        assert_eq!(existing[0]["description"], "new");
    }

    #[test]
    fn merge_skill_bindings_ignores_invalid_items_without_skill_id() {
        let mut existing = vec![serde_json::json!({
            "skill_id": "skill-a",
            "name": "Skill A",
            "status": "active",
            "description": "desc"
        })];
        let incoming = vec![serde_json::json!({
            "name": "Invalid Skill",
            "status": "active",
            "description": "invalid"
        })];

        merge_skill_bindings(&mut existing, incoming);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0]["skill_id"], "skill-a");
    }

    #[tokio::test]
    async fn create_starts_without_project_dir() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);

        let session = service
            .create(Some("s".to_string()), "agent-1".to_string(), String::new())
            .await?;

        let control = session.control.read().await;
        assert_eq!(control.project_dir, None);
        Ok(())
    }

    #[tokio::test]
    async fn create_for_agent_inherits_project_dir_only() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let inherited = dir.path().join("project-a");

        let session = service
            .create_for_agent(
                Some("s".to_string()),
                "agent-1".to_string(),
                String::new(),
                Some(inherited.clone()),
            )
            .await?;

        let control = session.control.read().await;
        assert_eq!(control.project_dir, Some(inherited));
        assert_eq!(control.active_agent, "agent-1");
        assert_eq!(control.skill_bindings.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn touch_session_refreshes_latest_agent_ordering() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

        let first = service
            .create(Some("first".to_string()), "agent-1".to_string(), String::new())
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let second = service
            .create(Some("second".to_string()), "agent-1".to_string(), String::new())
            .await?;

        service.touch_session(&first.id).await?;

        let latest = service
            .find_latest_session_by_agent("agent-1")
            .await?
            .expect("latest session should exist");
        assert_eq!(latest.id, first.id);
        assert_ne!(latest.id, second.id);

        let stored = repository
            .find_latest_session_by_agent("agent-1")
            .await?
            .expect("stored latest session should exist");
        assert_eq!(stored.0, first.id);
        Ok(())
    }

    #[tokio::test]
    async fn skill_bindings_are_persisted_after_service_rebuild() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

        let session = service
            .create(Some("s".to_string()), "agent-1".to_string(), String::new())
            .await?;
        service
            .update_runtime_state(
                &session.id,
                None,
                None,
                Some(vec![serde_json::json!({
                    "skill_id":"skill-a",
                    "name":"Skill A",
                    "status":"active",
                    "description": serde_json::Value::Null
                })]),
            )
            .await?;

        let rebuilt = SessionService::new(Arc::new(SessionCache::new()), repository);
        let loaded = rebuilt.get(&session.id).await?.expect("session should exist");
        let control = loaded.control.read().await;
        assert_eq!(control.skill_bindings.len(), 1);
        assert_eq!(control.skill_bindings[0]["skill_id"], "skill-a");
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_skill_updates_do_not_lose_or_duplicate_bindings() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);

        let session = service
            .create(Some("s".to_string()), "agent-1".to_string(), String::new())
            .await?;
        let session_id = session.id.clone();

        let service_left = service.clone();
        let service_right = service.clone();
        let left = tokio::spawn(async move {
            service_left
                .update_runtime_state(
                    &session_id,
                    None,
                    None,
                    Some(vec![serde_json::json!({
                        "skill_id":"skill-a",
                        "name":"Skill A",
                        "status":"active",
                        "description": serde_json::Value::Null
                    })]),
                )
                .await
        });

        let session_id_for_right = session.id.clone();
        let right = tokio::spawn(async move {
            service_right
                .update_runtime_state(
                    &session_id_for_right,
                    None,
                    None,
                    Some(vec![serde_json::json!({
                        "skill_id":"skill-b",
                        "name":"Skill B",
                        "status":"active",
                        "description": serde_json::Value::Null
                    })]),
                )
                .await
        });

        left.await??;
        right.await??;

        let loaded = service.get(&session.id).await?.expect("session should exist");
        let control = loaded.control.read().await;
        assert_eq!(control.skill_bindings.len(), 2);
        let mut ids = control
            .skill_bindings
            .iter()
            .filter_map(|value| value.get("skill_id").and_then(|skill_id| skill_id.as_str()))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(ids, vec!["skill-a", "skill-b"]);
        Ok(())
    }

    #[tokio::test]
    async fn provider_http_trace_bound_message_id_is_persisted() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

        let session = service
            .create(Some("s".to_string()), "agent-1".to_string(), String::new())
            .await?;

        let metadata = serde_json::json!({
            "providerHttpTrace": {
                "requestBody": {"foo":"bar"},
                "responseBody": {"id":"resp-1"},
                "format": "json",
                "boundMessageId": "",
                "capturedAt": 1,
                "truncated": false
            }
        });
        service
            .append_message(
                &session.id,
                crate::message::Role::Assistant,
                vec![crate::message::ContentBlock::Text {
                    text: "assistant".to_string(),
                }],
                Some(metadata),
            )
            .await?;

        let rebuilt = SessionService::new(Arc::new(SessionCache::new()), repository);
        let loaded = rebuilt
            .get_with_history(&session.id)
            .await?
            .expect("session should exist");
        let history = loaded.get_history().await;
        let assistant = history
            .iter()
            .find(|message| message.role == crate::message::Role::Assistant)
            .expect("assistant message should exist");
        let trace = assistant
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_http_trace.as_ref())
            .expect("provider trace should exist");
        assert_eq!(trace.bound_message_id, assistant.id);
        Ok(())
    }

    #[tokio::test]
    async fn default_title_is_used_when_create_name_missing() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;
        assert_eq!(session.get_name().await, super::DEFAULT_SESSION_TITLE);
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_starts_after_second_user_message_with_enough_chars() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.attempt_count, 0);
        }

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.status, crate::conversation::control::TitleStatus::Succeeded);
            assert_eq!(title_state.source, crate::conversation::control::TitleSource::Ai);
            assert_eq!(title_state.attempt_count, 1);
        }
        assert_ne!(session.get_name().await, super::DEFAULT_SESSION_TITLE);
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_waits_for_third_message_when_chars_not_enough() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "短句".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "再短".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.attempt_count, 0);
        }

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "第三条补充足够语义信息用于触发标题自动生成".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.attempt_count, 1);
            assert_eq!(title_state.status, crate::conversation::control::TitleStatus::Succeeded);
        }
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_retryable_error_marks_failed_and_increments_attempt() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new_with_title_generator(
            Arc::new(SessionCache::new()),
            repository,
            Arc::new(MockTitleGenerator {
                mode: MockTitleMode::RetryableError("network".to_string()),
            }),
        );
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Failed);
        assert_eq!(title_state.attempt_count, 1);
        assert!(title_state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("retryable"));
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_timeout_marks_failed_without_blocking_append() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new_with_title_generator(
            Arc::new(SessionCache::new()),
            repository,
            Arc::new(MockTitleGenerator {
                mode: MockTitleMode::Timeout,
            }),
        );
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        let started = std::time::Instant::now();
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        tokio::time::sleep(std::time::Duration::from_millis(
            super::TITLE_GENERATION_TIMEOUT_MS + 200,
        ))
        .await;

        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Failed);
        assert!(title_state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("timeout"));
        Ok(())
    }

    // Plan 3: 后端单元测试 - 标题状态机完整覆盖

    #[test]
    fn title_state_initializes_as_idle_with_default_source() {
        let title_state = TitleState::new_default();
        assert_eq!(title_state.status, TitleStatus::Idle);
        assert_eq!(title_state.source, TitleSource::Default);
        assert_eq!(title_state.attempt_count, 0);
        assert!(title_state.last_error.is_none());
        assert!(title_state.last_success_at.is_none());
    }

    #[test]
    fn title_state_set_pending_increments_attempt_and_records_timestamp() {
        let mut title_state = TitleState::new_default();
        let before = chrono::Utc::now().timestamp_millis();
        title_state.set_pending(2);
        let after = chrono::Utc::now().timestamp_millis();

        assert_eq!(title_state.status, TitleStatus::Pending);
        assert_eq!(title_state.attempt_count, 1);
        assert!(title_state.last_attempt_at >= before);
        assert!(title_state.last_attempt_at <= after);
        assert_eq!(title_state.based_on_user_message_count, 2);
    }

    #[test]
    fn title_state_set_succeeded_changes_source_to_ai() {
        let mut title_state = TitleState::new_default();
        title_state.set_pending(2);
        title_state.set_succeeded();

        assert_eq!(title_state.status, TitleStatus::Succeeded);
        assert_eq!(title_state.source, TitleSource::Ai);
        assert!(title_state.last_success_at.is_some());
        assert!(title_state.last_error.is_none());
    }

    #[test]
    fn title_state_set_failed_records_error_and_status() {
        let mut title_state = TitleState::new_default();
        title_state.set_pending(2);
        title_state.set_failed("network error".to_string());

        assert_eq!(title_state.status, TitleStatus::Failed);
        assert_eq!(title_state.last_error, Some("network error".to_string()));
    }

    #[test]
    fn title_state_should_retry_when_failed_and_under_max_attempts() {
        let mut title_state = TitleState::new_default();
        title_state.set_pending(2);
        title_state.set_failed("error".to_string());
        assert!(title_state.should_retry());
    }

    #[test]
    fn title_state_should_not_retry_when_already_succeeded() {
        let mut title_state = TitleState::new_default();
        title_state.set_pending(2);
        title_state.set_succeeded();
        assert!(!title_state.should_retry());
    }

    #[test]
    fn title_state_should_not_retry_when_max_attempts_reached() {
        let mut title_state = TitleState::new_default();
        title_state.attempt_count = super::TITLE_MAX_ATTEMPTS;
        title_state.status = TitleStatus::Failed;
        title_state.last_error = Some("error".to_string());
        assert!(!title_state.should_retry());
    }

    #[tokio::test]
    async fn title_generation_does_not_trigger_on_first_user_message() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let title_state = session.title_state.read().await;
        assert_eq!(title_state.attempt_count, 0);
        assert_eq!(title_state.status, TitleStatus::Idle);
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_does_not_trigger_on_short_messages() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "你好".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "继续".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let title_state = session.title_state.read().await;
        assert_eq!(title_state.attempt_count, 0);
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_succeeded_then_continues_stable() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        // First two messages trigger title
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.status, TitleStatus::Succeeded);
        }

        // Continue chatting - title should not change
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "再加一个定时任务功能".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        {
            let title_state = session.title_state.read().await;
            assert_eq!(title_state.status, TitleStatus::Succeeded);
            assert_eq!(title_state.attempt_count, 1);
        }
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_concurrent_append_only_triggers_once() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        // First message
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;

        // Two concurrent messages that should trigger title generation
        let session_id = session.id.clone();
        let service_clone = service.clone();
        let (tx1, rx1) = tokio::sync::oneshot::channel();
        let (tx2, rx2) = tokio::sync::oneshot::channel();

        let session_id_for_t1 = session_id.clone();
        let service_clone_for_t1 = service_clone.clone();
        let t1 = tokio::spawn(async move {
            let _ = service_clone_for_t1
                .append_message(
                    &session_id_for_t1,
                    crate::message::Role::User,
                    vec![crate::message::ContentBlock::Text {
                        text: "要支持重试队列".to_string(),
                    }],
                    None,
                )
                .await;
            tx1.send(()).unwrap();
        });
        let session_id_for_t2 = session_id.clone();
        let service_clone_for_t2 = service_clone.clone();
        let t2 = tokio::spawn(async move {
            let _ = service_clone_for_t2
                .append_message(
                    &session_id_for_t2,
                    crate::message::Role::User,
                    vec![crate::message::ContentBlock::Text {
                        text: "并且按项目分类展示".to_string(),
                    }],
                    None,
                )
                .await;
            tx2.send(()).unwrap();
        });

        rx1.await?;
        rx2.await?;
        t1.await?;
        t2.await?;

        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        let title_state = session.title_state.read().await;
        // Should only have one attempt, not two
        assert_eq!(title_state.attempt_count, 1);
        assert_eq!(title_state.status, TitleStatus::Succeeded);
        Ok(())
    }

    // Plan 4: 回归测试与稳定性验证

    #[tokio::test]
    async fn title_state_persists_after_service_rebuild() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        // Generate title
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        let original_title = session.get_name().await;
        let _original_title_state = session.title_state.read().await;

        // Simulate service rebuild by creating a new service with the same repository
        let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);

        // Load the session from the rebuilt service
        let loaded_session = rebuilt_service
            .get(&session.id)
            .await?
            .expect("session should exist after rebuild");

        let loaded_title_state = loaded_session.title_state.read().await;

        // Title state should persist after rebuild
        assert_eq!(loaded_title_state.status, TitleStatus::Succeeded);
        assert_eq!(loaded_title_state.source, TitleSource::Ai);
        assert!(loaded_title_state.last_success_at.is_some());
        assert!(loaded_title_state.last_error.is_none());
        assert!(loaded_title_state.attempt_count > 0);

        // Title name should also be preserved
        assert_eq!(loaded_session.get_name().await, original_title);
        Ok(())
    }

    #[tokio::test]
    async fn load_session_index_only_loads_metadata_until_history_requested() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "hello indexed world".to_string(),
                }],
                None,
            )
            .await?;

        let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);
        rebuilt_service.load_session_index().await?;

        let indexed = rebuilt_service.get(&session.id).await?.expect("session should exist");
        assert!(indexed.history.read().await.is_empty());

        let loaded = rebuilt_service
            .ensure_session_history_loaded(&session.id)
            .await?
            .expect("session should load history");
        assert!(loaded.history.read().await.len() >= 1);
        Ok(())
    }

    #[tokio::test]
    async fn ensure_session_history_loaded_deduplicates_concurrent_cold_loads() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "cold load one".to_string(),
                }],
                None,
            )
            .await?;

        let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);
        rebuilt_service.load_session_index().await?;

        let indexed = rebuilt_service.get(&session.id).await?.expect("session should exist");
        assert!(indexed.history.read().await.is_empty());

        let service_a = rebuilt_service.clone();
        let service_b = rebuilt_service.clone();
        let id_a = session.id.clone();
        let id_b = session.id.clone();

        let a = tokio::spawn(async move { service_a.ensure_session_history_loaded(&id_a).await });
        let b = tokio::spawn(async move { service_b.ensure_session_history_loaded(&id_b).await });

        let loaded_a = a.await??.expect("session should load");
        let loaded_b = b.await??.expect("session should load");

        assert!(loaded_a.history.read().await.len() >= 1);
        assert!(loaded_b.history.read().await.len() >= 1);
        assert!(rebuilt_service.cache.is_history_loaded(&session.id).await);
        Ok(())
    }

    #[tokio::test]
    async fn title_is_not_regenerated_after_success_and_reload() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        // First two messages trigger title generation
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Succeeded);
        assert_eq!(title_state.attempt_count, 1);

        // Simulate service restart: create a new service, load all sessions, then send a message
        let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);

        // Load session index only (simulating startup)
        rebuilt_service.load_session_index().await?;

        // Get the indexed session
        let loaded_session = rebuilt_service.get(&session.id).await?.expect("session should exist");

        // Send a new message after reload - title should NOT be regenerated
        let loaded_session_id = loaded_session.id.clone();
        rebuilt_service
            .append_message(
                &loaded_session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "再加一个定时任务功能".to_string(),
                }],
                None,
            )
            .await?;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let loaded_title_state = loaded_session.title_state.read().await;
        // Should still have only 1 attempt - no regeneration
        assert_eq!(loaded_title_state.attempt_count, 1);
        assert_eq!(loaded_title_state.status, TitleStatus::Succeeded);
        Ok(())
    }

    #[tokio::test]
    async fn title_generation_failure_does_not_leave_pending_state() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new_with_title_generator(
            Arc::new(SessionCache::new()),
            repository,
            Arc::new(MockTitleGenerator {
                mode: MockTitleMode::RetryableError("simulated failure".to_string()),
            }),
        );
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        // First two messages trigger title generation (will fail)
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        {
            let title_state = session.title_state.read().await;

            // After failure, status should NOT be Pending
            assert_ne!(title_state.status, TitleStatus::Pending);
            assert_eq!(title_state.status, TitleStatus::Failed);
            assert!(title_state
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("retryable"));
            assert!(title_state.last_error.is_some());
        }

        // Third message should trigger retry (not leave pending state)
        service
            .append_message(
                &session.id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "并且支持自动保存".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        {
            let title_state = session.title_state.read().await;
            // After retry, should still be Failed (not Pending)
            assert_ne!(title_state.status, TitleStatus::Pending);
            assert_eq!(title_state.attempt_count, 2);
        }
        Ok(())
    }

    #[tokio::test]
    async fn session_summary_updated_event_emitted_once_on_title_change() -> Result<()> {
        let dir = tempdir()?;
        let manager = SqliteManager::new(dir.path()).await?;
        let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
        let service = SessionService::new(Arc::new(SessionCache::new()), repository);
        let session = service
            .create_for_agent(None, "agent-1".to_string(), String::new(), None)
            .await?;

        let session_id = session.id.clone();
        let session_clone = session.clone();

        // Track how many times the title changes by monitoring title_state
        let title_change_count = Arc::new(Mutex::new(0));
        let last_title = Arc::new(Mutex::new(session.get_name().await));

        // Helper to check if title changed
        let title_change_count_clone = title_change_count.clone();
        let last_title_clone = last_title.clone();
        let check_title_change = || async {
            let current_title = session_clone.get_name().await;
            let mut last = last_title_clone.lock().unwrap();
            if current_title != *last {
                *title_change_count_clone.lock().unwrap() += 1;
                *last = current_title;
            }
        };

        // First message - no title change
        service
            .append_message(
                &session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "我想做一个桌面端任务调度工具".to_string(),
                }],
                None,
            )
            .await?;
        check_title_change().await;
        assert_eq!(*title_change_count.lock().unwrap(), 0);

        // Second message - title should be generated and changed
        service
            .append_message(
                &session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        check_title_change().await;
        assert_eq!(*title_change_count.lock().unwrap(), 1);

        // Third message - no title change (title already set)
        service
            .append_message(
                &session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "再加一个定时任务功能".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        check_title_change().await;
        assert_eq!(*title_change_count.lock().unwrap(), 1);

        // Fourth message - no title change
        service
            .append_message(
                &session_id,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "并且支持自动保存".to_string(),
                }],
                None,
            )
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        check_title_change().await;
        assert_eq!(*title_change_count.lock().unwrap(), 1);

        // Verify title state is consistent
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Succeeded);
        assert_eq!(title_state.source, TitleSource::Ai);
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProjectDirService for SessionService {
    async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        SessionService::get_project_dir(self, session_id).await
    }

    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf> {
        SessionService::set_project_dir(self, session_id, &project_dir).await
    }
}
