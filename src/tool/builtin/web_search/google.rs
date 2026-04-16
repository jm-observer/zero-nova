use crate::tool::builtin::web_search::types::{SearchBackend, SearchResult};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct GoogleBackend {
    api_key: String,
    endpoint: String,
    cx: String,
    client: Client,
}

impl GoogleBackend {
    pub fn new(api_key: String, endpoint: String, cx: String, client: Client) -> Self {
        Self {
            api_key,
            endpoint,
            cx,
            client,
        }
    }
}

#[async_trait]
impl SearchBackend for GoogleBackend {
    fn name(&self) -> &str {
        "Google"
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let resp = self
            .client
            .get(&self.endpoint)
            .query(&[
                ("q", query),
                ("key", &self.api_key),
                ("cx", &self.cx),
                ("num", &limit.to_string()),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Google Search API error: {}", err_text));
        }

        let data: Value = resp.json().await?;
        let mut results = Vec::new();

        if let Some(items) = data["items"].as_array() {
            for item in items {
                results.push(SearchResult {
                    title: item["title"].as_str().unwrap_or("No Title").to_string(),
                    url: item["link"].as_str().unwrap_or("No URL").to_string(),
                    snippet: item["snippet"].as_str().unwrap_or("").to_string(),
                });
            }
        }

        Ok(results)
    }
}
