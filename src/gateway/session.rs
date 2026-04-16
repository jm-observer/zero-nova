use crate::message::Message;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use uuid::Uuid;

/// 单个会话的详细信息与状态
pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,      // unix timestamp in milliseconds
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
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore {
    /// 创建一个新会话
    pub async fn create(&self, name: Option<String>) -> Arc<Session> {
        let id = Uuid::new_v4().to_string();
        self.create_with_id(id, name).await
    }

    pub async fn create_with_id(&self, id: String, name: Option<String>) -> Arc<Session> {
        let length = if id.len() > 8 { 8 } else { id.len() };
        let session_name = name.unwrap_or_else(|| format!("Session {}", &id[..length]));
        let created_at = Utc::now().timestamp_millis();

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

    /// 获取所有会话
    pub async fn get_all(&self) -> Vec<Arc<Session>> {
        let sessions = self.sessions.read().unwrap();
        sessions.values().cloned().collect()
    }
}
