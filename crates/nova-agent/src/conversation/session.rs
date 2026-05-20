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
    /// 父 Session id；None 表示根 Session。创建后不变更。
    pub parent_session_id: Option<String>,
    /// 父 Session history 中派生本 Session 的那条 ToolUse 的 id。创建后不变更。
    pub parent_tool_use_id: Option<String>,
    /// 直接子 Session id 列表（append-only）；load 时从 repository 回填。
    pub child_session_ids: RwLock<Vec<String>>,
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

    /// 内存侧追加子 Session id，去重。持久化关系由子行的 parent_session_id 列承载。
    pub async fn push_child(&self, child_id: &str) {
        let mut children = self.child_session_ids.write().await;
        if !children.iter().any(|id| id == child_id) {
            children.push(child_id.to_string());
        }
        self.touch_updated_at();
    }

    pub async fn get_child_ids(&self) -> Vec<String> {
        self.child_session_ids.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::control::{ControlState, TitleState};
    use super::Session;
    use std::sync::atomic::AtomicI64;
    use tokio::sync::{Mutex, RwLock};

    fn new_test_session(id: &str) -> Session {
        Session {
            control: RwLock::new(ControlState::new("test-agent")),
            id: id.to_string(),
            name: RwLock::new(String::new()),
            history: RwLock::new(Vec::new()),
            created_at: 0,
            updated_at: AtomicI64::new(0),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
            title_state: RwLock::new(TitleState::new_default()),
            parent_session_id: None,
            parent_tool_use_id: None,
            child_session_ids: RwLock::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn push_child_dedups() {
        let session = new_test_session("p1");
        session.push_child("c1").await;
        session.push_child("c1").await;
        session.push_child("c1").await;
        assert_eq!(session.get_child_ids().await, vec!["c1".to_string()]);
    }

    #[tokio::test]
    async fn push_child_preserves_order() {
        let session = new_test_session("p1");
        session.push_child("c1").await;
        session.push_child("c2").await;
        session.push_child("c3").await;
        assert_eq!(
            session.get_child_ids().await,
            vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]
        );
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
