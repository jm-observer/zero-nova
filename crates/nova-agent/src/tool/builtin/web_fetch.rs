use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
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
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::network::build_web_client()?,
        })
    }

    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

/// Provides a default constructor for `WebFetchTool`.
impl Default for WebFetchTool {
    fn default() -> Self {
        Self::with_client(Client::new())
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for fetching web content.
impl Tool for WebFetchTool {
    /// Returns the tool definition for web fetching.
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "WebFetch".to_string(),
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
            defer_loading: false,
        }
    }

    /// Executes the web fetch based on input parameters.
    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let url = input["url"].as_str().ok_or_else(|| anyhow!("Missing 'url' field"))?;
        let selector_str = input["selector"].as_str().unwrap_or("body");

        let resp = self.client.get(url).send().await?;
        if !resp.status().is_success() {
            return Ok(ToolOutput {
                content: format!("Failed to fetch URL: HTTP {}", resp.status()),
                is_error: true,
                child_session: None,
                images: Vec::new(),
            });
        }

        let html_content = resp.text().await?;
        let document = Html::parse_document(&html_content);

        let selector = Selector::parse(selector_str)
            .or_else(|_| Selector::parse("body"))
            .map_err(|e| anyhow!("Invalid selector: {e}"))?;

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
                child_session: None,
                images: Vec::new(),
            })
        } else {
            Ok(ToolOutput {
                content: truncate(final_text, 50_000),
                is_error: false,
                child_session: None,
                images: Vec::new(),
            })
        }
    }
}

/// Truncates a string to `max_len` bytes safely at a char boundary.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... [truncated]", &s[..end])
    } else {
        s.to_string()
    }
}
