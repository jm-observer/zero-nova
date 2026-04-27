use super::session::Session;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct SessionCache {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
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

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(id).cloned()
    }

    pub fn insert(&self, id: String, session: Arc<Session>) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id, session);
    }

    pub fn remove(&self, id: &str) -> Option<Arc<Session>> {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(id)
    }

    pub fn list(&self) -> Vec<Arc<Session>> {
        let sessions = self.sessions.read().unwrap();
        sessions.values().cloned().collect()
    }
}
