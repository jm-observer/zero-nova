use crate::conversation::control::ControlState;
use crate::conversation::repository::SqliteSessionRepository;
use crate::gateway::protocol::{ContentBlockDTO, MessageDTO, Session as SessionProtocol};
use crate::message::{ContentBlock, Message, Role};
use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 单个会话的详细信息与状态
pub struct Session {
    pub control: std::sync::RwLock<ControlState>,
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,
    pub cancellation_token: RwLock<Option<CancellationToken>>,
}

impl Session {
    pub fn append_user_message(&self, input: &str) {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: input.to_string(),
            }],
        };
        let mut history = self.history.write().unwrap();
        history.push(msg);
        self.touch_updated_at();
    }

    pub fn append_assistant_messages(&self, msgs: Vec<Message>) {
        let mut history = self.history.write().unwrap();
        history.extend(msgs);
        self.touch_updated_at();
    }

    pub fn get_history(&self) -> Vec<Message> {
        self.history.read().unwrap().clone()
    }

    pub fn get_messages_dto(&self) -> Vec<MessageDTO> {
        let history = self.history.read().unwrap();
        history
            .iter()
            .map(|m| MessageDTO {
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m
                    .content
                    .iter()
                    .map(|c| match c {
                        ContentBlock::Text { text } => ContentBlockDTO::Text { text: text.clone() },
                        ContentBlock::Thinking { thinking } => ContentBlockDTO::Thinking {
                            thinking: thinking.clone(),
                        },
                        ContentBlock::ToolUse { id, name, input } => ContentBlockDTO::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        },
                        ContentBlock::ToolResult {
                            tool_use_id,
                            output,
                            is_error,
                        } => ContentBlockDTO::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: output.clone(),
                            is_error: *is_error,
                        },
                    })
                    .collect(),
                timestamp: self.created_at,
            })
            .collect()
    }

    pub fn touch_updated_at(&self) {
        self.updated_at.store(Utc::now().timestamp_millis(), Ordering::SeqCst);
    }

    pub fn set_cancellation_token(&self, token: CancellationToken) {
        let mut ct = self.cancellation_token.write().unwrap();
        *ct = Some(token);
    }

    pub fn clear_cancellation_token(&self) {
        let mut ct = self.cancellation_token.write().unwrap();
        *ct = None;
    }

    pub fn take_cancellation_token(&self) -> Option<CancellationToken> {
        let mut ct = self.cancellation_token.write().unwrap();
        ct.take()
    }
}

/// 整合了 SQLite 持久化的 Session 存储库
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    repository: SqliteSessionRepository,
}

impl SessionStore {
    /// 创建新的 SessionStore，包含 SQLite 实例
    pub fn new(repository: SqliteSessionRepository) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            repository,
        }
    }

    /// 从数据库加载所有会话到内存
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
                let mut sessions = self.sessions.write().unwrap();
                sessions.insert(id, session);
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

        // Write-Through: Save to DB first
        self.repository
            .save_session(&session.id, &session.name, &agent_id, session.created_at, now)
            .await?;

        // Save initial messages
        for msg in session.get_history() {
            self.repository
                .save_message(&session.id, msg.role.clone(), msg.content.clone(), now)
                .await?;
        }

        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id, session.clone());
        Ok(session)
    }

    /// 根据 ID 获取会话 (Read-Through)
    pub async fn get(&self, id: &str) -> Result<Option<Arc<Session>>> {
        // 1. Try memory
        {
            let sessions = self.sessions.read().unwrap();
            if let Some(s) = sessions.get(id) {
                return Ok(Some(s.clone()));
            }
        }

        // 2. Try DB
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

            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(id, session.clone());
            return Ok(Some(session));
        }

        Ok(None)
    }

    /// 辅助方法：将新消息追加到历史并持久化
    pub async fn append_message(&self, session_id: &str, role: Role, content: Vec<ContentBlock>) -> Result<()> {
        let session = self.get(session_id).await?.context("Session not found")?;

        let now = Utc::now().timestamp_millis();

        // 1. Update Memory
        {
            let mut history = session.history.write().unwrap();
            history.push(Message {
                role: role.clone(),
                content: content.clone(),
            });
            session.touch_updated_at();
        }

        // 2. Update DB (Write-Through)
        self.repository.save_message(session_id, role, content, now).await?;

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

    /// 按 updated_at 降序返回会话摘要列表
    pub async fn list_sorted(&self) -> Vec<SessionProtocol> {
        let sessions = self.sessions.read().unwrap();
        let mut list: Vec<_> = sessions.values().cloned().collect();

        list.sort_by(|a, b| {
            b.updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.updated_at.load(Ordering::SeqCst))
        });

        list.into_iter()
            .map(|s| SessionProtocol {
                id: s.id.clone(),
                title: Some(s.name.clone()),
                agent_id: s.control.read().unwrap().active_agent.clone(),
                created_at: s.created_at,
                updated_at: s.updated_at.load(Ordering::SeqCst),
                message_count: s.history.read().unwrap().len(),
            })
            .collect()
    }

    /// 删除会话
    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Delete from DB
        self.repository.delete_session(id).await?;

        // Delete from Memory
        let mut sessions = self.sessions.write().unwrap();
        Ok(sessions.remove(id).is_some())
    }

    /// 复制并可选截断会话
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

        // Save to DB
        self.repository
            .save_session(&session.id, &session.name, &agent_id, session.created_at, now)
            .await?;

        // Save messages
        for msg in session.get_history() {
            self.repository
                .save_message(&session.id, msg.role.clone(), msg.content.clone(), now)
                .await?;
        }

        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(new_id, session.clone());
        Ok(Some(session))
    }

    pub async fn list_ids(&self) -> Vec<String> {
        let sessions = self.sessions.read().unwrap();
        sessions.keys().cloned().collect()
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        unimplemented!("Use SessionStore::new(repository) instead of Default")
    }
}
