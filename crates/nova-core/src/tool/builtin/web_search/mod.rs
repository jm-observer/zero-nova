use crate::tool::builtin::web_search::types::SearchBackend;
use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, error, info};
use reqwest::Client;
use serde_json::{json, Value};

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
    /// Creates a `WebSearchTool` from configuration.
    pub fn new(config: &crate::config::SearchConfig) -> Self {
        let client = Client::new();

        // 1. 优先处理显式指定的后端
        if let Some(backend) = &config.backend {
            match backend.to_lowercase().as_str() {
                "google" => {
                    if let (Some(api_key), Some(cx)) = (&config.google_api_key, &config.google_cx) {
                        let endpoint = config
                            .google_endpoint
                            .clone()
                            .unwrap_or_else(|| "https://www.googleapis.com/customsearch/v1".to_string());
                        info!("Web search backend selected: Google");
                        return Self {
                            backend: Box::new(GoogleBackend::new(api_key.clone(), endpoint, cx.clone(), client)),
                        };
                    } else {
                        error!("Explicitly selected Google backend but keys are missing. Falling back...");
                    }
                }
                "tavily" => {
                    if let Some(api_key) = &config.tavily_api_key {
                        info!("Web search backend selected: Tavily (API key present)");
                        return Self {
                            backend: Box::new(TavilyBackend::new(api_key.clone(), client)),
                        };
                    } else {
                        error!("Explicitly selected Tavily backend but API key is missing. Falling back...");
                    }
                }
                "duckduckgo" => {
                    info!("Web search backend selected: DuckDuckGo");
                    return Self {
                        backend: Box::new(DuckDuckGoBackend::new(client)),
                    };
                }
                _ => {
                    error!("Unknown search backend: {}. Falling back to priority logic.", backend);
                }
            }
        }

        // 2. 无显式指定或回退：按照优先级 Google > Tavily > DuckDuckGo
        // Google CSE
        if let (Some(api_key), Some(cx)) = (&config.google_api_key, &config.google_cx) {
            let endpoint = config
                .google_endpoint
                .clone()
                .unwrap_or_else(|| "https://www.googleapis.com/customsearch/v1".to_string());

            info!("Web search backend automatically initialized: Google");
            return Self {
                backend: Box::new(GoogleBackend::new(
                    api_key.clone(),
                    endpoint,
                    cx.clone(),
                    client.clone(),
                )),
            };
        }

        // Tavily
        if let Some(api_key) = &config.tavily_api_key {
            info!("Web search backend automatically initialized: Tavily (API key present)");
            return Self {
                backend: Box::new(TavilyBackend::new(api_key.clone(), client.clone())),
            };
        }

        // DuckDuckGo (Fallback)
        info!("Web search backend automatically initialized: DuckDuckGo");
        Self {
            backend: Box::new(DuckDuckGoBackend::new(client)),
        }
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
            name: "WebSearch".to_string(),
            description: description.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "count": { "type": "integer", "description": "Number of results (default 5, max 10)" }
                },
                "required": ["query"]
            }),
            defer_loading: false,
        }
    }

    /// Executes the web search based on the input query.
    async fn execute(&self, input: Value, _context: Option<crate::tool::ToolContext>) -> Result<ToolOutput> {
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
                Ok(ToolOutput {
                    content: format!("Web search failed ({}): {}", self.backend.name(), e),
                    is_error: true,
                })
            }
        }
    }
}
