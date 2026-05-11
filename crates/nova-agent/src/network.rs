use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

const PROVIDER_TIMEOUT_SECS: u64 = 120;
const WEB_TIMEOUT_SECS: u64 = 30;
const HEALTH_TIMEOUT_SECS: u64 = 5;
const WEB_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Clone)]
pub struct HttpClients {
    pub provider: Client,
    pub web: Client,
    pub health: Client,
}

impl HttpClients {
    pub fn new() -> Result<Self> {
        Ok(Self {
            provider: build_provider_client()?,
            web: build_web_client()?,
            health: build_health_client()?,
        })
    }
}

pub fn build_provider_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(PROVIDER_TIMEOUT_SECS))
        .build()
        .context("failed to build provider HTTP client")
}

pub fn build_web_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(WEB_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(WEB_USER_AGENT)
        .build()
        .context("failed to build web HTTP client")
}

pub fn build_health_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(HEALTH_TIMEOUT_SECS))
        .build()
        .context("failed to build health HTTP client")
}
