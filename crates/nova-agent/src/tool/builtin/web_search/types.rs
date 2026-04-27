use anyhow::Result;
use async_trait::async_trait;

/// Unified search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search backend trait
#[async_trait]
pub trait SearchBackend: Send + Sync {
    /// Return backend name, used for logging and dynamic description generation
    fn name(&self) -> &str;

    /// Execute search, returning a list of unified results
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
}
