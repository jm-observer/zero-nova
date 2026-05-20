use super::super::session::Session;
use super::SessionService;
use anyhow::Result;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl SessionService {
    /// 持久化完整会话快照（用于 create/copy/rebuild 等路径，非常规热写入路径）。
    pub(super) async fn persist_full_session(&self, session: &Arc<Session>) -> Result<()> {
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.get_name().await,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
                session.parent_session_id.as_deref(),
                session.parent_tool_use_id.as_deref(),
            )
            .await?;

        for msg in session.get_history().await {
            self.repository
                .save_message(
                    &session.id,
                    &msg.id,
                    msg.role.clone(),
                    msg.content.clone(),
                    msg.metadata.as_ref().map(serde_json::to_value).transpose()?,
                    msg.created_at,
                )
                .await?;
        }

        Ok(())
    }

    pub(super) async fn persist_session_control(&self, session: &Arc<Session>) -> Result<()> {
        sync_title_state_into_control(session).await;
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };

        self.repository
            .save_session(
                &session.id,
                &session.get_name().await,
                &runtime_control.active_agent,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
                &runtime_control,
                session.parent_session_id.as_deref(),
                session.parent_tool_use_id.as_deref(),
            )
            .await
    }

    pub(super) async fn persist_runtime_control(&self, session_id: &str, session: &Arc<Session>) -> Result<()> {
        sync_title_state_into_control(session).await;
        let runtime_control = {
            let control = session.control.read().await;
            control.clone()
        };
        let updated_at = chrono::Utc::now().timestamp_millis();
        session.updated_at.store(updated_at, Ordering::SeqCst);

        self.repository
            .update_session_runtime_control(session_id, &runtime_control, updated_at)
            .await
    }
}

async fn sync_title_state_into_control(session: &Arc<Session>) {
    let title_state = session.title_state.read().await.clone();
    let mut control = session.control.write().await;
    control.title_state = title_state;
}
