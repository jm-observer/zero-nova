use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

/// Search backend type
enum SearchProvider {
    Google,
    Brave,
}

/// Tool for performing web searches.
pub struct WebSearchTool {
    api_key: String,
    endpoint: String,
    cx: Option<String>, // Required for Google
    provider: SearchProvider,
    client: Client,
}

/// Implementation of methods for the web search tool.
impl WebSearchTool {
    /// Constructs a new `WebSearchTool`.
    pub fn new(api_key: String, endpoint: String, cx: Option<String>, provider: SearchProvider) -> Self {
        Self {
            api_key,
            endpoint,
            cx,
            provider,
            client: Client::new(),
        }
    }

    /// Creates a `WebSearchTool` from environment variables.
    /// Prioritizes Google Programmable Search, falls back to Brave Search.
    pub fn from_env() -> Result<Self> {
        // Try Google first
        if let Ok(api_key) = std::env::var("GOOGLE_SEARCH_API_KEY") {
            let cx = std::env::var("GOOGLE_SEARCH_CX").map_err(|_| anyhow!("GOOGLE_SEARCH_CX not set"))?;
            let endpoint = std::env::var("GOOGLE_SEARCH_ENDPOINT")
                .unwrap_or_else(|_| "https://www.googleapis.com/customsearch/v1".to_string());
            return Ok(Self::new(api_key, endpoint, Some(cx), SearchProvider::Google));
        }

        // Fallback to Brave
        let api_key = std::env::var("SEARCH_API_KEY").map_err(|_| anyhow!("Neither GOOGLE_SEARCH_API_KEY nor SEARCH_API_KEY set"))?;
        let endpoint = std::env::var("SEARCH_ENDPOINT")
            .unwrap_or_else(|_| "https://api.search.brave.com/res/v1/web/search".to_string());
        Ok(Self::new(api_key, endpoint, None, SearchProvider::Brave))
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for web search.
impl Tool for WebSearchTool {
    /// Returns the tool definition for the web search tool.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information. Currently configured to search specific high-quality sources like GitHub and Hugging Face if using Google backend.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "count": { "type": "integer", "description": "Number of results (default 5, max 10)" }
                },
                "required": ["query"]
            }),
        }
    }

    /// Executes the web search based on the input query.
    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'query' field"))?;
        let count = input["count"].as_u64().unwrap_or(5).min(10);

        let mut request = self.client.get(&self.endpoint);

        match self.provider {
            SearchProvider::Google => {
                let cx = self.cx.as_ref().ok_or_else(|| anyhow!("CX not configured for Google search"))?;
                request = request.query(&[
                    ("q", query),
                    ("key", &self.api_key),
                    ("cx", cx),
                    ("num", &count.to_string()),
                ]);
            }
            SearchProvider::Brave => {
                request = request
                    .query(&[("q", query), ("count", &count.to_string())])
                    .header("x-subscription-token", &self.api_key)
                    .header("Accept", "application/json");
            }
        }

        let resp = request.send().await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Ok(ToolOutput {
                content: format!("Search API error ({}): {}", self.endpoint, err_text),
                is_error: true,
            });
        }

        let data: Value = resp.json().await?;
        let mut results = String::new();
        results.push_str(&format!("Search results for \"{}\":\n\n", query));

        match self.provider {
            SearchProvider::Google => {
                if let Some(items) = data["items"].as_array() {
                    for (i, res) in items.iter().enumerate() {
                        let title = res["title"].as_str().unwrap_or("No Title");
                        let url = res["link"].as_str().unwrap_or("No URL");
                        let snip = res["snippet"].as_str().unwrap_or("");
                        results.push_str(&format!("{}. [{}]({})\n   {}\n\n", i + 1, title, url, snip));
                    }
                } else {
                    results.push_str("No results found.\n");
                }
            }
            SearchProvider::Brave => {
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
            }
        }

        Ok(ToolOutput {
            content: results,
            is_error: false,
        })
    }
}
