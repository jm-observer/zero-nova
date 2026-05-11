use super::SessionService;
use crate::conversation::session::SessionSummary;
use anyhow::{Context, Result};
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl SessionService {
    /// 启动阶段仅加载会话索引（不加载消息历史）。
    pub async fn load_session_index(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, title, agent_id, created_at, updated_at, runtime_control) in rows {
            let title_state = runtime_control.title_state.clone();
            let session = Arc::new(
                super::session_from_index_row(
                    id.clone(),
                    title,
                    agent_id,
                    created_at,
                    updated_at,
                    runtime_control,
                    title_state,
                )
                .await,
            );
            self.cache.insert_indexed(id, session).await;
        }
        Ok(())
    }

    /// 从数据库加载所有会话到内存（完整 history，测试/迁移辅助）。
    pub async fn load_all(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (id, _title, _agent_id, _created_at, _updated_at, _runtime_control) in rows {
            if let Some(session) = self.load_session_from_db(&id).await? {
                self.cache.insert_loaded(id, session).await;
            }
        }
        Ok(())
    }

    pub async fn find_latest_session_by_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        let Some((session_id, _title, _agent_id, _created_at, _updated_at, _runtime_control)) =
            self.repository.find_latest_session_by_agent(agent_id).await?
        else {
            return Ok(None);
        };

        self.get(&session_id).await
    }

    /// 获取会话元数据（可能未加载 history）。
    pub async fn get(&self, id: &str) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        if let Some(session) = self.cache.get(id).await {
            return Ok(Some(session));
        }

        let loaded = self.repository.load_session_meta(id).await?;
        let Some((id, title, agent_id, created_at, updated_at, runtime_control)) = loaded else {
            return Ok(None);
        };

        let title_state = runtime_control.title_state.clone();
        let session = Arc::new(
            super::session_from_index_row(
                id.clone(),
                title,
                agent_id,
                created_at,
                updated_at,
                runtime_control,
                title_state,
            )
            .await,
        );
        self.cache.insert_indexed(id, session.clone()).await;
        Ok(Some(session))
    }

    /// 获取会话并确保历史消息已加载（同 session 并发去重）。
    pub async fn get_with_history(&self, id: &str) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        self.ensure_session_history_loaded(id).await
    }

    pub async fn ensure_session_history_loaded(
        &self,
        id: &str,
    ) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        if self.cache.is_history_loaded(id).await {
            return Ok(self.cache.get(id).await);
        }

        if self.cache.get(id).await.is_none() {
            if self.get(id).await?.is_none() {
                return Ok(None);
            }
            if self.cache.is_history_loaded(id).await {
                return Ok(self.cache.get(id).await);
            }
        }

        let mut receiver = None;
        let is_loader = {
            let mut loading = self.loading.write().await;
            if let Some(waiters) = loading.get_mut(id) {
                let (tx, rx) = tokio::sync::oneshot::channel();
                waiters.push(tx);
                receiver = Some(rx);
                false
            } else {
                loading.insert(id.to_string(), Vec::new());
                true
            }
        };

        if !is_loader {
            if let Some(rx) = receiver {
                match rx.await {
                    Ok(session) => return Ok(session),
                    Err(_) => {
                        if self.cache.is_history_loaded(id).await {
                            return Ok(self.cache.get(id).await);
                        }
                    }
                }
            }
            return Ok(self.cache.get(id).await);
        }

        let load_result = self.load_session_from_db(id).await?;
        if let Some(session) = load_result.as_ref() {
            self.cache.replace_with_loaded(id.to_string(), session.clone()).await;
        }

        let waiters = {
            let mut loading = self.loading.write().await;
            loading.remove(id).unwrap_or_default()
        };
        for waiter in waiters {
            let _ = waiter.send(load_result.clone());
        }

        Ok(load_result)
    }

    pub async fn list_sorted(&self) -> Vec<SessionSummary> {
        let mut entries = self.cache.list_entries().await;

        entries.sort_by(|a, b| {
            b.session
                .updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.session.updated_at.load(Ordering::SeqCst))
        });

        let mut summaries = Vec::with_capacity(entries.len());
        for entry in entries {
            let session = entry.session;
            let name = session.get_name().await;
            let agent_id = session.control.read().await.active_agent.clone();
            let message_count = if entry.history_loaded {
                session.history.read().await.len()
            } else {
                0
            };
            summaries.push(SessionSummary {
                id: session.id.clone(),
                name,
                agent_id,
                created_at: session.created_at,
                updated_at: session.updated_at.load(Ordering::SeqCst),
                message_count,
            });
        }
        summaries
    }

    pub async fn get_project_dir(&self, session_id: &str) -> Result<Option<std::path::PathBuf>> {
        let session = self.get(session_id).await?.context("Session not found")?;
        let control = session.control.read().await;
        Ok(control.project_dir.clone())
    }

    pub(super) async fn load_session_from_db(
        &self,
        id: &str,
    ) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        let loaded = self.repository.load_session(id).await?;
        Ok(loaded.map(
            |(id, title, _agent_id, created_at, updated_at, runtime_control, history)| {
                let title_state = runtime_control.title_state.clone();
                Arc::new(crate::conversation::session::Session {
                    control: tokio::sync::RwLock::new(runtime_control),
                    id,
                    name: tokio::sync::RwLock::new(title),
                    history: tokio::sync::RwLock::new(history),
                    created_at,
                    updated_at: std::sync::atomic::AtomicI64::new(updated_at),
                    chat_lock: tokio::sync::Mutex::new(()),
                    cancellation_token: tokio::sync::RwLock::new(None),
                    title_state: tokio::sync::RwLock::new(title_state),
                })
            },
        ))
    }
}
