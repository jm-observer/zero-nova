pub mod commands;
pub mod events;
mod helpers;
mod persist;
pub mod queries;
mod session_factory;
mod skill_bindings;
#[cfg(test)]
mod tests;
mod title;
mod types;
mod write;

use super::cache::SessionCache;
use super::repository::SqliteSessionRepository;
use crate::tool::ProjectDirService;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use types::LoadingWaiters;

// 标题生成常量
/// 首次尝试触发标题生成的最小用户消息数
pub const TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT: usize = 2;
/// 第二次尝试触发标题生成的最小用户消息数
pub const TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT: usize = 3;
/// 最大尝试次数
pub const TITLE_MAX_ATTEMPTS: u8 = 2;
/// 最小总字符数（所有用户文本消息的字符总和）
pub const TITLE_MIN_TOTAL_CHARS: usize = 24;
/// 标题生成超时时间
pub const TITLE_GENERATION_TIMEOUT_MS: u64 = 3_000;

/// 默认会话标题
const DEFAULT_SESSION_TITLE: &str = "未命名会话";

#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    /// De-duplicates concurrent cold loads for the same session id.
    loading: Arc<RwLock<LoadingWaiters>>,
}

impl SessionService {
    pub fn new(cache: Arc<SessionCache>, repository: SqliteSessionRepository) -> Self {
        Self {
            cache,
            repository,
            loading: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_repository(&self) -> SqliteSessionRepository {
        self.repository.clone()
    }
}

#[async_trait::async_trait]
impl ProjectDirService for SessionService {
    async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>> {
        SessionService::get_project_dir(self, session_id).await
    }

    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf> {
        SessionService::set_project_dir(self, session_id, &project_dir).await
    }
}
