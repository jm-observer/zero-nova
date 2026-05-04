use super::cache::SessionCache;
use super::control::ControlState;
use super::repository::SqliteSessionRepository;
use super::session::{Session, SessionSummary};
use crate::message::{ContentBlock, Message, Role};
use crate::tool::ProjectDirService;
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

type SessionLoadResult = Option<Arc<Session>>;
type LoadingWaiters = HashMap<String, Vec<oneshot::Sender<SessionLoadResult>>>;

#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    /// De-duplicates concurrent cold loads for the same session id.
    loading: Arc<RwLock<LoadingWaiters>>,
}

impl SessionService {
    pub fn new(cache: Arc<SessionCache>, repository: SqliteSessionRepository) -> Self {
        Self {
            cache,
            repository,
            loading: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_repository(&self) -> SqliteSessionRepository {
        self.repository.clone()
    }

    /// 从数据库加载所有会话到内存 (仅启动阶段使用)
    pub async fn load_all(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, _title, _agent_id, _created_at, _updated_at, _runtime_control) in rows {
            if let Some(session) = self.load_session_from_db(&id).await? {
                self.cache.insert(id, session);
            }
        }
        Ok(())
    }

    /// 创建一个新会话并持久化
    pub async fn create(&self, name: Option<String>, agent_id: String, system_prompt: String) -> Result<Arc<Session>> {
        let id = Uuid::new_v4().to_string();
        let length = id.len().min(8);
        let session_name = name.unwrap_or_else(|| format!("Session {}", &id[..length]));
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
            control: std::sync::RwLock::new(ControlState::new(&agent_id)),
            id: id.clone(),
            name: session_name,
            history: RwLock::new(initial_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        self.persist_full_session(&session).await?;
        self.cache.insert(id, session.clone());
        Ok(session)
    }

    /// 获取会话 (Read-Through with concurrency protection).
    pub async fn get(&self, id: &str) -> Result<Option<Arc<Session>>> {
        if let Some(session) = self.cache.get(id) {
            return Ok(Some(session));
        }

        let mut receiver = None;
        let is_loader = {
            let mut loading = self.loading.write().unwrap_or_else(|poisoned| poisoned.into_inner());
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
                        if let Some(session) = self.cache.get(id) {
                            return Ok(Some(session));
                        }
                    }
                }
            }
            return Ok(None);
        }

        let load_result = self.load_session_from_db(id).await?;
        if let Some(session) = load_result.as_ref() {
            self.cache.insert(id.to_string(), session.clone());
        }

        let waiters = {
            let mut loading = self.loading.write().unwrap_or_else(|poisoned| poisoned.into_inner());
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
                Arc::new(Session {
                    control: std::sync::RwLock::new(runtime_control),
                    id,
                    name: title,
                    history: RwLock::new(history),
                    created_at,
                    updated_at: AtomicI64::new(updated_at),
                    chat_lock: Mutex::new(()),
                    cancellation_token: RwLock::new(None),
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
        let session = self.get(session_id).await?.context("Session not found")?;
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
            let mut history = session.history.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            history.push(Message {
                id: message_id.clone(),
                role: role.clone(),
                content: content.clone(),
                created_at: now,
                metadata: parsed_metadata,
            });
            session.touch_updated_at();
        }

        self.repository
            .save_message(session_id, &message_id, role, content, persisted_metadata, now)
            .await?;
        self.persist_session_control(&session).await?;

        Ok(())
    }

    pub async fn list_sorted(&self) -> Vec<SessionSummary> {
        let mut list = self.cache.list();

        list.sort_by(|a, b| {
            b.updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.updated_at.load(Ordering::SeqCst))
        });

        list.into_iter()
            .map(|session| SessionSummary {
                id: session.id.clone(),
                name: session.name.clone(),
                agent_id: session
                    .control
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_agent
                    .clone(),
                created_at: session.created_at,
                updated_at: session.updated_at.load(Ordering::SeqCst),
                message_count: session
                    .history
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .len(),
            })
            .collect()
    }

    pub async fn set_active_agent(&self, session_id: &str, agent_id: &str) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.active_agent = agent_id.to_string();
        }

        self.persist_session_control(&session).await?;
        Ok(session)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.repository.delete_session(id).await?;
        Ok(self.cache.remove(id).is_some())
    }

    pub async fn copy_session(&self, source_id: &str, truncate_index: Option<usize>) -> Result<Option<Arc<Session>>> {
        let source = self.get(source_id).await?.context("Source session not found")?;

        let history = source.get_history();
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
            let source_control = source.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            let agent_id = source_control.active_agent.clone();
            let mut new_control = ControlState::new_with_project_dir(&agent_id, source_control.project_dir.clone());
            new_control.model_override = source_control.model_override.clone();
            new_control
        };

        let session = Arc::new(Session {
            control: std::sync::RwLock::new(new_control),
            id: new_id.clone(),
            name: format!("{} (Copy)", source.name),
            history: RwLock::new(new_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        self.persist_full_session(&session).await?;
        self.cache.insert(new_id, session.clone());
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
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.model_override.orchestration = orchestration;
            control.model_override.execution = execution;
            control.model_override.updated_at = Utc::now().timestamp_millis();
        }

        self.persist_runtime_control(session_id, &session).await?;
        Ok(session)
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
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(control.project_dir.clone())
    }

    pub async fn set_project_dir(&self, session_id: &str, project_dir: &Path) -> Result<PathBuf> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let normalized = normalize_project_dir(project_dir).await;

        {
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
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

    async fn persist_full_session(&self, session: &Arc<Session>) -> Result<()> {
        let runtime_control = {
            let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.name,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
            )
            .await?;

        for msg in session.get_history() {
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
        let runtime_control = {
            let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.name,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
            )
            .await
    }

    async fn persist_runtime_control(&self, session_id: &str, session: &Arc<Session>) -> Result<()> {
        let runtime_control = {
            let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.clone()
        };

        self.repository
            .update_session_runtime_control(session_id, &runtime_control)
            .await
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
    use crate::conversation::sqlite_manager::SqliteManager;
    use anyhow::Result;
    use std::sync::Arc;
    use tempfile::tempdir;

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

        let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(control.project_dir, None);
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
        let control = loaded.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let control = loaded.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let loaded = rebuilt.get(&session.id).await?.expect("session should exist");
        let history = loaded.get_history();
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
}

async fn normalize_project_dir(path: &Path) -> PathBuf {
    match tokio::fs::canonicalize(path).await {
        Ok(canonical) => canonical,
        Err(err) => {
            log::warn!(
                "Failed to canonicalize project_dir '{}': {}. Using raw path.",
                path.display(),
                err
            );
            path.to_path_buf()
        }
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
