use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;

use nova_agent::message::Message;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::control::ControlState;

/// Session represents a single conversation turn with its own lock.
///
/// Uses `std::sync::RwLock` because:
/// - Lock hold time is short (<1ms for small vec clone)
/// - Each session is accessed by a single async task at a time (via `chat_lock`)
/// - Blocking in the async thread is acceptable for this usage pattern
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
        self.history
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn get_internal_messages(&self) -> Vec<Message> {
        self.history
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn touch_updated_at(&self) {
        self.updated_at
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
    }

    pub fn set_cancellation_token(&self, token: CancellationToken) {
        let mut ct = self
            .cancellation_token
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *ct = Some(token);
    }

    pub fn clear_cancellation_token(&self) {
        let mut ct = self
            .cancellation_token
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *ct = None;
    }

    pub fn take_cancellation_token(&self) -> Option<CancellationToken> {
        let mut ct = self
            .cancellation_token
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ct.take()
    }

    pub fn get_active_agent(&self) -> String {
        self.control
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_agent
            .clone()
    }

    pub fn set_active_agent(&self, agent_id: &str) {
        let mut control = self.control.write().unwrap_or_else(|poisoned| poisoned.into_inner());
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
