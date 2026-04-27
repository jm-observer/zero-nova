use crate::tool::builtin::web_search::types::{SearchBackend, SearchResult};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

pub struct DuckDuckGoBackend {
    client: Client,
}

impl DuckDuckGoBackend {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SearchBackend for DuckDuckGoBackend {
    fn name(&self) -> &str {
        "DuckDuckGo"
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let params = [("q", query)];
        let resp = self.client
            .post("https://html.duckduckgo.com/html/")
            .form(&params)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("DuckDuckGo HTML search failed with status: {}", resp.status()));
        }

        let body = resp.text().await?;
        let document = Html::parse_document(&body);

        let result_container_selector = Selector::parse(".result").map_err(|e| anyhow!("Selector error: {}", e))?;
        let link_selector = Selector::parse(".result__a").map_err(|e| anyhow!("Selector error: {}", e))?;
        let snippet_selector = Selector::parse(".result__snippet").map_err(|e| anyhow!("Selector error: {}", e))?;

        let mut final_results = Vec::new();

        for container in document.select(&result_container_selector) {
            if final_results.len() >= limit {
                break;
            }

            if let Some(link_el) = container.select(&link_selector).next() {
                let title = link_el.text().collect::<Vec<_>>().join("").trim().to_string();
                let url = link_el.value().attr("href").unwrap_or("").to_string();
                let snippet = container
                    .select(&snippet_selector)
                    .next()
                    .map(|el| el.text().collect::<String>())
                    .unwrap_or_default();

                if !title.is_empty() && !url.is_empty() {
                    final_results.push(SearchResult { title, url, snippet });
                }
            }
        }

        Ok(final_results)
    }
}
