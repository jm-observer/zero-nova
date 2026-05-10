use std::sync::atomic::{AtomicI64, Ordering};

use crate::message::Message;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::control::{ControlState, TitleState};

pub struct Session {
    pub control: RwLock<ControlState>,
    pub id: String,
    pub name: RwLock<String>,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,
    pub cancellation_token: RwLock<Option<CancellationToken>>,
    /// 标题生成状态（内存中维护，不直接持久化到 SQLite）。
    /// 解释 `name` 是默认值还是 AI 生成结果。
    pub title_state: RwLock<TitleState>,
}

impl Session {
    pub async fn get_name(&self) -> String {
        self.name.read().await.clone()
    }

    pub async fn set_name(&self, name: String) {
        let mut current = self.name.write().await;
        *current = name;
    }

    pub async fn get_history(&self) -> Vec<Message> {
        self.history.read().await.clone()
    }

    pub async fn get_internal_messages(&self) -> Vec<Message> {
        self.history.read().await.clone()
    }

    pub fn touch_updated_at(&self) {
        self.updated_at
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
    }

    pub async fn set_cancellation_token(&self, token: CancellationToken) {
        let mut ct = self.cancellation_token.write().await;
        *ct = Some(token);
    }

    pub async fn clear_cancellation_token(&self) {
        let mut ct = self.cancellation_token.write().await;
        *ct = None;
    }

    pub async fn take_cancellation_token(&self) -> Option<CancellationToken> {
        let mut ct = self.cancellation_token.write().await;
        ct.take()
    }

    pub async fn get_active_agent(&self) -> String {
        self.control.read().await.active_agent.clone()
    }

    pub async fn set_active_agent(&self, agent_id: &str) {
        let mut control = self.control.write().await;
        control.active_agent = agent_id.to_string();
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}
