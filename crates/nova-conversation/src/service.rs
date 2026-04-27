use crate::cache::SessionCache;
use crate::control::ControlState;
use crate::repository::SqliteSessionRepository;
use crate::session::{Session, SessionSummary};
use anyhow::{Context, Result};
use chrono::Utc;
use nova_core::message::{ContentBlock, Message, Role};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

/// Guard for an in-flight session load, used to prevent race conditions.
pub enum LoadingGuard {
    /// A session is currently being loaded; join this to get the result.
    InFlight(oneshot::Receiver<Arc<Session>>),
    /// Session already exists in cache (fast path after insert).
    Ready(Arc<Session>),
}

#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    /// Tracks in-flight session loads. Used to de-duplicate concurrent cold loads
    /// for the same session ID in read-through mode.
    loading: Arc<RwLock<HashMap<String, oneshot::Sender<Arc<Session>>>>>,
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
            if let Ok(Some((id, title, _agent_id, created_at, updated_at, runtime_control, history))) =
                self.repository.load_session(&id).await
            {
                let session = Arc::new(Session {
                    control: std::sync::RwLock::new(runtime_control),
                    id: id.clone(),
                    name: title,
                    history: RwLock::new(history),
                    created_at,
                    updated_at: AtomicI64::new(updated_at),
                    chat_lock: Mutex::new(()),
                    cancellation_token: RwLock::new(None),
                });
                self.cache.insert(id, session);
            }
        }
        Ok(())
    }

    /// 创建一个新会话并持久化
    pub async fn create(&self, name: Option<String>, agent_id: String, system_prompt: String) -> Result<Arc<Session>> {
        let id = Uuid::new_v4().to_string();
        let length = if id.len() > 8 { 8 } else { id.len() };
        let session_name = name.unwrap_or_else(|| format!("Session {}", &id[..length]));
        let now = Utc::now().timestamp_millis();

        let mut initial_history = Vec::new();
        if !system_prompt.is_empty() {
            initial_history.push(Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: system_prompt }],
            });
        }

        let session = Arc::new(Session {
            control: std::sync::RwLock::new(ControlState::new(&agent_id)),
            id: id.clone(),
            name: session_name.clone(),
            history: RwLock::new(initial_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        // 持久化到 DB
        let rc = {
            let control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.clone()
        };
        self.repository
            .save_session(&session.id, &session.name, &agent_id, session.created_at, now, &rc)
            .await?;

        // 持久化初始消息
        for msg in session.get_history() {
            self.repository
                .save_message(&session.id, msg.role.clone(), msg.content.clone(), now)
                .await?;
        }

        self.cache.insert(id, session.clone());
        Ok(session)
    }

    /// 获取会话 (Read-Through with concurrency protection).
    ///
    /// Prevents a race condition where multiple concurrent callers load the same
    /// session from the database simultaneously, creating duplicate `Arc<Session>` entries.
    pub async fn get(&self, id: &str) -> Result<Option<Arc<Session>>> {
        // Fast path: check cache
        if let Some(s) = self.cache.get(id) {
            return Ok(Some(s));
        }

        let id_owned: String = id.to_string();

        // Try to register as the loader. If `tx` was already in the map, we lost the race.
        let (wrapper_tx, wrapper_rx) = oneshot::channel();
        let was_loaded = {
            let mut loading = self.loading.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            // Wrap the real tx we'll use later
            loading.insert(id_owned.clone(), wrapper_tx)
        };

        match was_loaded {
            Some(_prev_tx) => {
                // We lost the race: await the receiver from the first-ordered caller.
                // The first-ordered caller will send via its `tx`, and our `rx` receives it.
                if let Ok(s) = wrapper_rx.await {
                    self.cache.insert(id_owned, s.clone());
                    return Ok(Some(s));
                }
                // Cache check for second-ordered fallback
                if let Some(s) = self.cache.get(id) {
                    return Ok(Some(s));
                }
                Ok(None)
            }
            None => {
                // We are the loader: load from DB
                let session = self.load_session_from_db(id).await?;
                if let Some(ref s) = session {
                    self.cache.insert(id_owned, s.clone());
                }
                Ok(session)
            }
        }
    }

    /// Load a single session from the database.
    async fn load_session_from_db(&self, id: &str) -> Result<Option<Arc<Session>>> {
        if let Ok(Some((id, title, _agent_id, created_at, updated_at, runtime_control, history))) =
            self.repository.load_session(id).await
        {
            let session = Arc::new(Session {
                control: std::sync::RwLock::new(runtime_control),
                id: id.clone(),
                name: title,
                history: RwLock::new(history),
                created_at,
                updated_at: AtomicI64::new(updated_at),
                chat_lock: Mutex::new(()),
                cancellation_token: RwLock::new(None),
            });
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    pub async fn append_message(&self, session_id: &str, role: Role, content: Vec<ContentBlock>) -> Result<()> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let now = Utc::now().timestamp_millis();

        // 1. 更新内存
        {
            let mut history = session.history.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            history.push(Message {
                role: role.clone(),
                content: content.clone(),
            });
            session.touch_updated_at();
        }

        // 2. 持久化消息
        self.repository.save_message(session_id, role, content, now).await?;

        // 3. 更新会话元数据
        let rc = {
            let runtime_control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime_control.clone()
        };
        self.repository
            .save_session(
                &session.id,
                &session.name,
                &rc.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &rc,
            )
            .await?;

        Ok(())
    }

    pub async fn list_sorted(&self) -> Vec<SessionSummary> {
        let mut list: Vec<_> = self.cache.list();

        list.sort_by(|a, b| {
            b.updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.updated_at.load(Ordering::SeqCst))
        });

        list.into_iter()
            .map(|s| SessionSummary {
                id: s.id.clone(),
                name: s.name.clone(),
                agent_id: s
                    .control
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_agent
                    .clone(),
                created_at: s.created_at,
                updated_at: s.updated_at.load(Ordering::SeqCst),
                message_count: s.history.read().unwrap_or_else(|poisoned| poisoned.into_inner()).len(),
            })
            .collect()
    }

    pub async fn set_active_agent(&self, session_id: &str, agent_id: &str) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.active_agent = agent_id.to_string();
        }

        let rc = {
            let runtime_control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime_control.clone()
        };
        self.repository
            .save_session(
                &session.id,
                &session.name,
                agent_id,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &rc,
            )
            .await?;

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
        let agent_id = source.control.read().unwrap().active_agent.clone();
        let mut new_control = ControlState::new(&agent_id);
        {
            let source_control = source.control.read().unwrap();
            new_control.model_override = source_control.model_override.clone();
        }
        let new_name = format!("{} (Copy)", source.name);

        let session = Arc::new(Session {
            control: std::sync::RwLock::new(new_control.clone()),
            id: new_id.clone(),
            name: new_name.clone(),
            history: RwLock::new(new_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        self.repository
            .save_session(
                &session.id,
                &session.name,
                &agent_id,
                session.created_at,
                now,
                &new_control,
            )
            .await?;

        for msg in session.get_history() {
            self.repository
                .save_message(&session.id, msg.role.clone(), msg.content.clone(), now)
                .await?;
        }

        self.cache.insert(new_id, session.clone());
        Ok(Some(session))
    }

    pub async fn override_model(
        &self,
        session_id: &str,
        orchestration: Option<crate::control::ModelRef>,
        execution: Option<crate::control::ModelRef>,
    ) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            control.model_override.orchestration = orchestration;
            control.model_override.execution = execution;
            control.model_override.updated_at = Utc::now().timestamp_millis();
        }

        let rc = {
            let runtime_control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime_control.clone()
        };
        self.repository.update_session_runtime_control(session_id, &rc).await?;

        Ok(session)
    }

    pub async fn update_runtime_state(
        &self,
        session_id: &str,
        snapshot: Option<crate::control::LastTurnSnapshot>,
        token_delta: Option<(u64, u64, u64, u64)>,
    ) -> Result<()> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(s) = snapshot {
                control.last_turn_snapshot = Some(s);
            }
            if let Some((input, output, cache_creation, cache_read)) = token_delta {
                control.token_counters.input_tokens += input;
                control.token_counters.output_tokens += output;
                control.token_counters.cache_creation_input_tokens += cache_creation;
                control.token_counters.cache_read_input_tokens += cache_read;
                control.token_counters.updated_at = Utc::now().timestamp_millis();
            }
        }

        let rc = {
            let runtime_control = session.control.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime_control.clone()
        };
        self.repository.update_session_runtime_control(session_id, &rc).await?;

        Ok(())
    }
}
