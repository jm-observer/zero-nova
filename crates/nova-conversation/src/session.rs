use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;

use nova_core::message::Message;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::control::ControlState;

pub struct Session {
    pub control: RwLock<ControlState>,
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,
    pub cancellation_token: RwLock<Option<CancellationToken>>,
}

impl Session {
    pub fn get_history(&self) -> Vec<Message> {
        self.history.read().unwrap().clone()
    }

    pub fn get_internal_messages(&self) -> Vec<Message> {
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

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}
