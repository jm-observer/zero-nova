use crate::tool::builtin::web_search::types::{SearchBackend, SearchResult};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{error, info};
use reqwest::Client;
use serde_json::Value;

pub struct TavilyBackend {
    api_key: String,
    client: Client,
}

impl TavilyBackend {
    pub fn new(api_key: String, client: Client) -> Self {
        Self { api_key, client }
    }
}

#[async_trait]
impl SearchBackend for TavilyBackend {
    fn name(&self) -> &str {
        "Tavily"
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let body = serde_json::json!({
            "api_key": self.api_key,
            "query": query,
            "max_results": limit,
            "search_depth": "basic"
        });

        info!("Tavily request body: {}", body);

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            error!("Tavily API error response: {}", err_text);
            return Err(anyhow!("Tavily API error: {}", err_text));
        }

        let data: Value = resp.json().await?;
        let mut results = Vec::new();

        if let Some(items) = data["results"].as_array() {
            for item in items {
                info!("Tavily response: {item:?}");
                results.push(SearchResult {
                    title: item["title"].as_str().unwrap_or("No Title").to_string(),
                    url: item["url"].as_str().unwrap_or("No URL").to_string(),
                    snippet: item["content"].as_str().unwrap_or("").to_string(),
                });
            }
        }

        Ok(results)
    }
}
