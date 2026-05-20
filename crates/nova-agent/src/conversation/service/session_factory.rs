use super::super::control::{ControlState, TitleState};
use super::super::session::Session;
use std::sync::atomic::AtomicI64;
use tokio::sync::{Mutex, RwLock};

#[allow(clippy::too_many_arguments)]
pub(super) async fn session_from_index_row(
    id: String,
    title: String,
    agent_id: String,
    created_at: i64,
    updated_at: i64,
    runtime_control: ControlState,
    title_state: TitleState,
    parent_session_id: Option<String>,
    parent_tool_use_id: Option<String>,
    child_session_ids: Vec<String>,
) -> Session {
    let session = Session {
        control: tokio::sync::RwLock::new(runtime_control),
        id,
        name: RwLock::new(title),
        history: RwLock::new(Vec::new()),
        created_at,
        updated_at: AtomicI64::new(updated_at),
        chat_lock: Mutex::new(()),
        cancellation_token: RwLock::new(None),
        title_state: RwLock::new(title_state),
        parent_session_id,
        parent_tool_use_id,
        child_session_ids: RwLock::new(child_session_ids),
    };
    {
        let mut control = session.control.write().await;
        if control.active_agent.is_empty() {
            control.active_agent = agent_id;
        }
    }
    session
}
