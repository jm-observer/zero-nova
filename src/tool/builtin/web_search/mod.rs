use crate::tool::builtin::web_search::types::{SearchBackend, SearchResult};
use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use log::{debug, error, info};
use reqwest::Client;
use serde_json::{Value, json};
use std::env;

pub mod duckduckgo;
pub mod google;
pub mod tavily;
pub mod types;

use self::duckduckgo::DuckDuckGoBackend;
use self::google::GoogleBackend;
use self::tavily::TavilyBackend;

/// Reconstructed WebSearchTool using the Strategy Pattern.
pub struct WebSearchTool {
    backend: Box<dyn SearchBackend>,
}

impl WebSearchTool {
    /// Creates a `WebSearchTool` from environment variables.
    /// Implements priority: Google > Tavily > DuckDuckGo.
    pub fn from_env() -> Result<Self> {
        let client = Client::new();

        // 1. Google CSE
        if let Ok(api_key) = env::var("GOOGLE_SEARCH_API_KEY") {
            let cx = env::var("GOOGLE_SEARCH_CX").map_err(|_| anyhow!("GOOGLE_SEARCH_CX not set"))?;
            let endpoint = env::var("GOOGLE_SEARCH_ENDPOINT")
                .unwrap_or_else(|_| "https://www.googleapis.com/customsearch/v1".to_string());

            let backend = Box::new(GoogleBackend::new(api_key, endpoint, cx, client.clone()));
            info!("Web search backend initialized: Google");
            return Ok(Self { backend });
        }

        // 2. Tavily
        if let Ok(api_key) = env::var("TAVILY_API_KEY") {
            let backend = Box::new(TavilyBackend::new(api_key, client.clone()));
            info!("Web search backend initialized: Tavily");
            return Ok(Self { backend });
        }

        // 3. DuckDuckGo (Fallback)
        let backend = Box::new(DuckDuckGoBackend::new(client.clone()));
        info!("Web search backend initialized: DuckDuckGo");
        Ok(Self { backend })
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    /// Returns the tool definition for the web search tool.
    fn definition(&self) -> ToolDefinition {
        let backend_name = self.backend.name();
        let description = match backend_name {
            "Google" => "Search the web via Google. Focused on GitHub, HuggingFace, docs.rs if using Google backend.",
            "Tavily" => "Search the web via Tavily. Results are optimized for LLM consumption.",
            _ => "Search the web via DuckDuckGo. No API key required.",
        };

        ToolDefinition {
            name: "web_search".to_string(),
            description: description.to_string(),
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

        let start_time = std::time::Instant::now();
        debug!("Web search started: query=\"{}\", limit={}", query, count);

        let results_result = self.backend.search(query, count as usize).await;

        let duration = start_time.elapsed();

        match results_result {
            Ok(results) => {
                info!(
                    "Web search succeeded: backend=\"{}\", query=\"{}\", results={}, duration={:?}",
                    self.backend.name(),
                    query,
                    results.len(),
                    duration
                );

                if results.is_empty() {
                    return Ok(ToolOutput {
                        content: format!("Search results for \"{}\":\n\nNo results found.\n", query),
                        is_error: false,
                    });
                }

                let mut content = format!("Search results for \"{}\":\n\n", query);
                for (i, res) in results.iter().enumerate() {
                    content.push_str(&format!(
                        "{}. [{}]({})\n   {}\n\n",
                        i + 1,
                        res.title,
                        res.url,
                        res.snippet
                    ));
                }

                Ok(ToolOutput {
                    content,
                    is_error: false,
                })
            }
            Err(e) => {
                error!(
                    "Web search failed: backend=\"{}\", query=\"{}\", error=\"{}\", duration={:?}",
                    self.backend.name(),
                    query,
                    e,
                    duration
                );
                Err(e)
            }
        }
    }
}
