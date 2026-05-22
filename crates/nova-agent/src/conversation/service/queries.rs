use super::SessionService;
use crate::conversation::session::SessionSummary;
use anyhow::{anyhow, Context, Result};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// 祖先链 walk 的深度上限，防环 / 防退化。skill 委派实际只有 1~2 层。
const MAX_ANCESTOR_WALK_DEPTH: usize = 64;

impl SessionService {
    /// 启动阶段仅加载会话索引（不加载消息历史）。
    pub async fn load_session_index(&self) -> Result<()> {
        let rows = self.repository.list_sessions().await?;
        for (
            id,
            title,
            agent_id,
            created_at,
            updated_at,
            runtime_control,
            parent_session_id,
            parent_tool_use_id,
            root_session_id,
            ancestor_ids,
        ) in rows
        {
            let title_state = runtime_control.title_state.clone();
            // 回填 child_session_ids（load 路径，create 路径初始为空——见 plan-1 步骤 5）。
            let child_session_ids = self.repository.list_child_session_ids(&id).await.unwrap_or_default();
            let session = Arc::new(
                super::session_factory::session_from_index_row(
                    id.clone(),
                    title,
                    agent_id,
                    created_at,
                    updated_at,
                    runtime_control,
                    title_state,
                    parent_session_id,
                    parent_tool_use_id,
                    root_session_id,
                    ancestor_ids,
                    child_session_ids,
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
        for (id, ..) in rows {
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
        let Some((session_id, ..)) = self.repository.find_latest_session_by_agent(agent_id).await? else {
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
        let Some((
            id,
            title,
            agent_id,
            created_at,
            updated_at,
            runtime_control,
            parent_session_id,
            parent_tool_use_id,
            root_session_id,
            ancestor_ids,
        )) = loaded
        else {
            return Ok(None);
        };

        let title_state = runtime_control.title_state.clone();
        let child_session_ids = self.repository.list_child_session_ids(&id).await.unwrap_or_default();
        let session = Arc::new(
            super::session_factory::session_from_index_row(
                id.clone(),
                title,
                agent_id,
                created_at,
                updated_at,
                runtime_control,
                title_state,
                parent_session_id,
                parent_tool_use_id,
                root_session_id,
                ancestor_ids,
                child_session_ids,
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

    /// 按 parent_session_id 列出直接子 Session 的轻量摘要（不加载 history）。
    /// message_count 走单独的 COUNT 查询。Plan 3 对外 `list_child_sessions` 委托此方法。
    pub async fn list_child_session_summaries(&self, parent_id: &str) -> Result<Vec<SessionSummary>> {
        let child_ids = self.repository.list_child_session_ids(parent_id).await?;
        let mut out = Vec::with_capacity(child_ids.len());
        for id in child_ids {
            if let Some(row) = self.repository.load_session_meta(&id).await? {
                let msg_count = self.repository.count_messages(&id).await.unwrap_or(0);
                out.push(SessionSummary {
                    id: row.0,
                    name: row.1,
                    agent_id: row.2,
                    created_at: row.3,
                    updated_at: row.4,
                    message_count: msg_count,
                });
            }
        }
        Ok(out)
    }

    pub(super) async fn load_session_from_db(
        &self,
        id: &str,
    ) -> Result<Option<Arc<crate::conversation::session::Session>>> {
        let loaded = self.repository.load_session(id).await?;
        let Some((
            row_id,
            title,
            _agent_id,
            created_at,
            updated_at,
            runtime_control,
            history,
            parent_session_id,
            parent_tool_use_id,
            root_session_id,
            ancestor_ids,
        )) = loaded
        else {
            return Ok(None);
        };

        let child_session_ids = self
            .repository
            .list_child_session_ids(&row_id)
            .await
            .unwrap_or_default();
        let title_state = runtime_control.title_state.clone();
        Ok(Some(Arc::new(crate::conversation::session::Session {
            control: tokio::sync::RwLock::new(runtime_control),
            id: row_id,
            name: tokio::sync::RwLock::new(title),
            history: tokio::sync::RwLock::new(history),
            created_at,
            updated_at: std::sync::atomic::AtomicI64::new(updated_at),
            chat_lock: tokio::sync::Mutex::new(()),
            cancellation_token: tokio::sync::RwLock::new(None),
            title_state: tokio::sync::RwLock::new(title_state),
            parent_session_id,
            parent_tool_use_id,
            root_session_id,
            ancestor_ids,
            child_session_ids: tokio::sync::RwLock::new(child_session_ids),
        })))
    }

    /// 解析 session 的顶层 root session id。
    /// 优先读 root_session_id 列；列为 None（v0.3.14 前的存量行）时降级沿
    /// parent_session_id 链 walk 到顶。session 不存在返回 Err。
    pub async fn resolve_session_root(&self, session_id: &str) -> Result<String> {
        let mut current = session_id.to_string();
        for _ in 0..MAX_ANCESTOR_WALK_DEPTH {
            let meta = self
                .repository
                .load_session_meta(&current)
                .await?
                .with_context(|| format!("session not found: {current}"))?;
            // meta: (id, title, agent_id, created_at, updated_at, runtime_control,
            //        parent_session_id, parent_tool_use_id, root_session_id, ancestor_ids)
            let (.., parent_session_id, _, root_session_id, _) = meta;
            if let Some(root) = root_session_id {
                return Ok(root);
            }
            match parent_session_id {
                Some(parent) => current = parent,
                None => return Ok(current), // 无 parent 即根
            }
        }
        Err(anyhow!("ancestor chain too deep or cyclic from {session_id}"))
    }

    /// 解析完整祖先链（根在前→直接父在后）。优先读 ancestor_ids 列；
    /// 列为 None（存量行）时降级 walk。根 session 返回空 Vec。
    pub async fn resolve_session_ancestors(&self, session_id: &str) -> Result<Vec<String>> {
        let meta = self
            .repository
            .load_session_meta(session_id)
            .await?
            .with_context(|| format!("session not found: {session_id}"))?;
        let (.., parent_session_id, _, _, ancestor_ids) = meta;
        if let Some(ancestors) = ancestor_ids {
            return Ok(ancestors);
        }
        // 存量行：沿 parent 链 walk，收集顺序为 直接父→…→根，最后反转。
        let mut chain = Vec::new();
        let mut current = parent_session_id;
        while let Some(pid) = current {
            if chain.len() >= MAX_ANCESTOR_WALK_DEPTH {
                return Err(anyhow!("ancestor chain too deep or cyclic from {session_id}"));
            }
            let pmeta = self
                .repository
                .load_session_meta(&pid)
                .await?
                .with_context(|| format!("session not found: {pid}"))?;
            chain.push(pid);
            let (.., pparent, _, _, _) = pmeta;
            current = pparent;
        }
        chain.reverse();
        Ok(chain)
    }
}
