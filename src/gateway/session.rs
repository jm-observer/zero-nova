use crate::message::Message;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

/// 单个会话的详细信息与状态
pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: String,   // unix timestamp string
    pub chat_lock: Mutex<()>, // 确保同一会话内的聊天请求串行执行
}

/// 内存会话存储库
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionStore {
    /// 创建新的 SessionStore
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// 创建一个新会话
    pub async fn create(&self, name: Option<String>) -> Arc<Session> {
        let id = Uuid::new_v4().to_string();
        let session_name = name.unwrap_or_else(|| format!("Session {}", id[..8].to_string()));
        let created_at = Utc::now().timestamp().to_string();

        let session = Arc::new(Session {
            id: id.clone(),
            name: session_name,
            history: RwLock::new(Vec::new()),
            created_at,
            chat_lock: Mutex::new(()),
        });

        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id, session.clone());
        session
    }

    /// 根据 ID 获取会话
    pub async fn get(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(id).cloned()
    }

    /// 列出所有会话的简要信息
    pub async fn list(&self) -> Vec<crate::gateway::protocol::SessionInfo> {
        let sessions = self.sessions.read().unwrap();
        sessions
            .values()
            .map(|s| {
                let history = s.history.read().unwrap();
                crate::gateway::protocol::SessionInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    message_count: history.len(),
                    created_at: s.created_at.clone(),
                }
            })
            .collect()
    }
}
