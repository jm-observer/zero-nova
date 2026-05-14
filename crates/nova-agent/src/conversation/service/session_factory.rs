use super::super::control::{ControlState, TitleState};
use super::super::session::Session;
use std::sync::atomic::AtomicI64;
use tokio::sync::{Mutex, RwLock};

pub(super) async fn session_from_index_row(
    id: String,
    title: String,
    agent_id: String,
    created_at: i64,
    updated_at: i64,
    runtime_control: ControlState,
    title_state: TitleState,
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
    };
    {
        let mut control = session.control.write().await;
        if control.active_agent.is_empty() {
            control.active_agent = agent_id;
        }
    }
    session
}
