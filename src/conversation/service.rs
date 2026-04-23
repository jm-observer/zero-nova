use crate::conversation::cache::SessionCache;
use crate::conversation::control::ControlState;
use crate::conversation::repository::SqliteSessionRepository;
use crate::conversation::session::{Session, SessionSummary};
use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
}

impl SessionService {
    pub fn new(cache: Arc<SessionCache>, repository: SqliteSessionRepository) -> Self {
        Self { cache, repository }
    }

    /// 从数据库加载所有会话到内存 (仅启动阶段使用)
    pub async fn load_all(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, _title, _agent_id, _created_at, _updated_at) in rows {
            if let Ok(Some((id, title, agent_id, created_at, updated_at, history))) =
                self.repository.load_session(&id).await
            {
                let session = Arc::new(Session {
                    control: std::sync::RwLock::new(ControlState::new(&agent_id)),
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
        self.repository
            .save_session(&session.id, &session.name, &agent_id, session.created_at, now)
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

    /// 获取会话 (Read-Through)
    pub async fn get(&self, id: &str) -> Result<Option<Arc<Session>>> {
        // 1. 尝试缓存
        if let Some(s) = self.cache.get(id) {
            return Ok(Some(s));
        }

        // 2. 尝试 DB
        if let Ok(Some((id, title, agent_id, created_at, updated_at, history))) = self.repository.load_session(id).await
        {
            let session = Arc::new(Session {
                control: std::sync::RwLock::new(ControlState::new(&agent_id)),
                id: id.clone(),
                name: title,
                history: RwLock::new(history),
                created_at,
                updated_at: AtomicI64::new(updated_at),
                chat_lock: Mutex::new(()),
                cancellation_token: RwLock::new(None),
            });

            self.cache.insert(id, session.clone());
            return Ok(Some(session));
        }

        Ok(None)
    }

    pub async fn append_message(&self, session_id: &str, role: Role, content: Vec<ContentBlock>) -> Result<()> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let now = Utc::now().timestamp_millis();

        // 1. 更新内存
        {
            let mut history = session.history.write().unwrap();
            history.push(Message {
                role: role.clone(),
                content: content.clone(),
            });
            session.touch_updated_at();
        }

        // 2. 持久化消息
        self.repository.save_message(session_id, role, content, now).await?;

        // 3. 更新会话元数据
        let active_agent = session.control.read().unwrap().active_agent.clone();
        self.repository
            .save_session(
                &session.id,
                &session.name,
                &active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
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
                agent_id: s.control.read().unwrap().active_agent.clone(),
                created_at: s.created_at,
                updated_at: s.updated_at.load(Ordering::SeqCst),
                message_count: s.history.read().unwrap().len(),
            })
            .collect()
    }

    pub async fn set_active_agent(&self, session_id: &str, agent_id: &str) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap();
            control.active_agent = agent_id.to_string();
        }

        self.repository
            .save_session(
                &session.id,
                &session.name,
                agent_id,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
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
        let new_name = format!("{} (Copy)", source.name);

        let session = Arc::new(Session {
            control: std::sync::RwLock::new(ControlState::new(&agent_id)),
            id: new_id.clone(),
            name: new_name.clone(),
            history: RwLock::new(new_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        self.repository
            .save_session(&session.id, &session.name, &agent_id, session.created_at, now)
            .await?;

        for msg in session.get_history() {
            self.repository
                .save_message(&session.id, msg.role.clone(), msg.content.clone(), now)
                .await?;
        }

        self.cache.insert(new_id, session.clone());
        Ok(Some(session))
    }
}
