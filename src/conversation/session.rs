use crate::conversation::control::ControlState;
use crate::message::Message;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// 单个会话的详细信息与状态
pub struct Session {
    pub control: std::sync::RwLock<ControlState>,
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    /// 串行执行锁，确保同一会话不会同时运行多个 Turn
    pub chat_lock: Mutex<()>,
    /// 当前正在运行的取消令牌
    pub cancellation_token: RwLock<Option<CancellationToken>>,
}

impl Session {
    pub fn get_history(&self) -> Vec<Message> {
        self.history.read().unwrap().clone()
    }

    pub(crate) fn get_internal_messages(&self) -> Vec<Message> {
        self.history.read().unwrap().clone()
    }

    pub fn touch_updated_at(&self) {
        self.updated_at
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
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

/// 内部表示用的会话摘要
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}
