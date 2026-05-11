use super::session::Session;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct CachedSessionEntry {
    pub session: Arc<Session>,
    pub history_loaded: bool,
}

pub struct SessionCache {
    sessions: RwLock<HashMap<String, CachedSessionEntry>>,
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.read().await;
        sessions.get(id).map(|entry| entry.session.clone())
    }

    pub async fn get_entry(&self, id: &str) -> Option<CachedSessionEntry> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    pub async fn insert(&self, id: String, session: Arc<Session>) {
        self.insert_loaded(id, session).await;
    }

    pub async fn insert_indexed(&self, id: String, session: Arc<Session>) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            id,
            CachedSessionEntry {
                session,
                history_loaded: false,
            },
        );
    }

    pub async fn insert_loaded(&self, id: String, session: Arc<Session>) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            id,
            CachedSessionEntry {
                session,
                history_loaded: true,
            },
        );
    }

    pub async fn replace_with_loaded(&self, id: String, session: Arc<Session>) {
        self.insert_loaded(id, session).await;
    }

    pub async fn is_history_loaded(&self, id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.get(id).map(|entry| entry.history_loaded).unwrap_or(false)
    }

    pub async fn remove(&self, id: &str) -> Option<Arc<Session>> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id).map(|entry| entry.session)
    }

    pub async fn list(&self) -> Vec<Arc<Session>> {
        let sessions = self.sessions.read().await;
        sessions.values().map(|entry| entry.session.clone()).collect()
    }

    pub async fn list_entries(&self) -> Vec<CachedSessionEntry> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }
}
