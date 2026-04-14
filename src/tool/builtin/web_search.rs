use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

pub struct WebSearchTool {
    api_key: String,
    endpoint: String,
    client: Client,
}

impl WebSearchTool {
    pub fn new(api_key: String, endpoint: String) -> Self {
        Self {
            api_key,
            endpoint,
            client: Client::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("SEARCH_API_KEY").map_err(|_| anyhow!("SEARCH_API_KEY not set"))?;
        let endpoint = std::env::var("SEARCH_ENDPOINT")
            .unwrap_or_else(|_| "https://api.search.brave.com/res/v1/web/search".to_string());
        Ok(Self::new(api_key, endpoint))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information using a search engine.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "count": { "type": "integer", "description": "Number of results (default 5, max 20)" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'query' field"))?;
        let count = input["count"].as_u64().unwrap_or(5).min(20);

        // This implementation currently assumes Brave Search API
        let resp = self
            .client
            .get(&self.endpoint)
            .query(&[("q", query), ("count", &count.to_string())])
            .header("x-subscription-token", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(ToolOutput {
                content: format!("Search API error: {}", resp.status()),
                is_error: true,
            });
        }

        let data: Value = resp.json().await?;

        // Extract results (Brave format)
        let mut results = String::new();
        results.push_str(&format!("Search results for \"{}\":\n\n", query));

        if let Some(web_results) = data["web"]["results"].as_array() {
            for (i, res) in web_results.iter().enumerate() {
                let title = res["title"].as_str().unwrap_or("No Title");
                let url = res["url"].as_str().unwrap_or("No URL");
                let snip = res["description"].as_str().unwrap_or("");
                results.push_str(&format!("{}. [{}]({})\n   {}\n\n", i + 1, title, url, snip));
            }
        } else {
            results.push_str("No results found.\n");
        }

        Ok(ToolOutput {
            content: results,
            is_error: false,
        })
    }
}
