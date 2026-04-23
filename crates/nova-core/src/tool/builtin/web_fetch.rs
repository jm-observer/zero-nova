use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::{json, Value};

/// Tool for fetching a URL and extracting text content.
pub struct WebFetchTool {
    client: Client,
}

/// Implementation of methods for `WebFetchTool`.
impl WebFetchTool {
    /// Creates a new `WebFetchTool` with a configured HTTP client.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(5))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

/// Provides a default constructor for `WebFetchTool`.
impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for fetching web content.
impl Tool for WebFetchTool {
    /// Returns the tool definition for web fetching.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Fetch a URL and extract its text content. Useful for reading web pages or documentation."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" },
                    "selector": { "type": "string", "description": "Optional CSS selector to extract specific content (e.g. 'article', '.main-content')" }
                },
                "required": ["url"]
            }),
        }
    }

    /// Executes the web fetch based on input parameters.
    async fn execute(&self, input: Value, _context: Option<crate::tool::ToolContext>) -> Result<ToolOutput> {
        let url = input["url"].as_str().ok_or_else(|| anyhow!("Missing 'url' field"))?;
        let selector_str = input["selector"].as_str().unwrap_or("body");

        let resp = self.client.get(url).send().await?;
        if !resp.status().is_success() {
            return Ok(ToolOutput {
                content: format!("Failed to fetch URL: HTTP {}", resp.status()),
                is_error: true,
            });
        }

        let html_content = resp.text().await?;
        let document = Html::parse_document(&html_content);

        let selector = match Selector::parse(selector_str) {
            Ok(s) => s,
            Err(_) => Selector::parse("body").unwrap(),
        };

        let mut text_output = String::new();
        for element in document.select(&selector) {
            for text in element.text() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    text_output.push_str(trimmed);
                    text_output.push(' ');
                }
            }
            text_output.push('\n');
        }

        let final_text = text_output.trim();
        if final_text.is_empty() {
            Ok(ToolOutput {
                content: "Fetched page but found no text content.".to_string(),
                is_error: true,
            })
        } else {
            Ok(ToolOutput {
                content: truncate(final_text, 50_000),
                is_error: false,
            })
        }
    }
}

/// Truncates a string to a maximum length, adding an ellipsis.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}... [truncated]", &s[..max_len])
    } else {
        s.to_string()
    }
}
