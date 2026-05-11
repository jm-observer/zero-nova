use crate::config::AppConfig;
use anyhow::Result;
use chrono::Utc;
use nova_protocol::observability::{ProviderHealthSnapshot, ProviderHealthSnapshotResponse};
use reqwest::{header, Client, StatusCode};
use std::time::Instant;
use tokio::task::JoinHandle;

const PROVIDER_HEALTH_DEGRADED_MS: u64 = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderKind {
    OpenAiCompat,
    Anthropic,
}

pub async fn collect_provider_health(config: &AppConfig) -> Result<ProviderHealthSnapshotResponse> {
    let checked_at = Utc::now().timestamp_millis();
    let providers: Vec<JoinHandle<ProviderHealthSnapshot>> = config
        .providers
        .iter()
        .map(|(scope, config)| {
            let provider_kind = infer_provider_kind(&config.base_url);
            let scope = scope.clone();
            let base_url = config.base_url.clone();
            let api_key = config.api_key.clone();
            tokio::spawn(async move {
                let probe = probe_provider_by_url(&base_url, &api_key, provider_kind).await;
                ProviderHealthSnapshot {
                    provider: scope.clone(),
                    scope: scope.to_string(),
                    status: probe.status.clone(),
                    checked_at,
                    latency_ms: probe.latency_ms,
                    message: probe.message.clone(),
                }
            })
        })
        .collect();

    let mut snapshots = Vec::with_capacity(providers.len());
    for handle in providers {
        match handle.await {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(error) => snapshots.push(ProviderHealthSnapshot {
                provider: "unknown".to_string(),
                scope: "unknown".to_string(),
                status: "unknown".to_string(),
                checked_at,
                latency_ms: None,
                message: Some(format!("Provider health task join failed: {error}")),
            }),
        }
    }

    Ok(ProviderHealthSnapshotResponse {
        providers: snapshots,
        updated_at: checked_at,
    })
}

async fn probe_provider_by_url(base_url: &str, api_key: &str, provider_kind: ProviderKind) -> HealthProbeResult {
    let client = match crate::network::build_health_client() {
        Ok(client) => client,
        Err(error) => {
            return HealthProbeResult::unreachable(format!("Failed to build HTTP client: {error}"));
        }
    };

    let url = build_probe_url(base_url.trim(), provider_kind);
    let started_at = Instant::now();
    let response = match build_probe_request(&client, &url, provider_kind, api_key).send().await {
        Ok(response) => response,
        Err(error) => return classify_transport_error(error),
    };

    let latency_ms = started_at.elapsed().as_millis() as u64;
    classify_status(response.status(), latency_ms)
}

fn build_probe_request<'a>(
    client: &'a Client,
    url: &'a str,
    provider_kind: ProviderKind,
    api_key: &'a str,
) -> reqwest::RequestBuilder {
    match provider_kind {
        ProviderKind::OpenAiCompat => client
            .get(url)
            .header(header::AUTHORIZATION, format!("Bearer {api_key}")),
        ProviderKind::Anthropic => client
            .get(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    }
}

fn classify_status(status: StatusCode, latency_ms: u64) -> HealthProbeResult {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return HealthProbeResult::auth_failed(format!("Provider returned HTTP {}", status.as_u16()));
    }

    if status.is_success() {
        let health_status = if latency_ms > PROVIDER_HEALTH_DEGRADED_MS {
            "degraded"
        } else {
            "healthy"
        };
        return HealthProbeResult {
            status: health_status.to_string(),
            latency_ms: Some(latency_ms),
            message: Some(format!("Provider returned HTTP {}", status.as_u16())),
        };
    }

    if status.is_client_error() {
        return HealthProbeResult::misconfigured(format!("Provider returned HTTP {}", status.as_u16()));
    }

    HealthProbeResult::unreachable(format!("Provider returned HTTP {}", status.as_u16()))
}

fn classify_transport_error(error: reqwest::Error) -> HealthProbeResult {
    if error.is_timeout() {
        return HealthProbeResult::unreachable("Provider request timed out");
    }

    if error.is_connect() {
        return HealthProbeResult::unreachable(format!("Provider connection failed: {error}"));
    }

    HealthProbeResult::unreachable(format!("Provider request failed: {error}"))
}

fn infer_provider_kind(base_url: &str) -> ProviderKind {
    if base_url.to_ascii_lowercase().contains("anthropic") {
        ProviderKind::Anthropic
    } else {
        ProviderKind::OpenAiCompat
    }
}

fn build_probe_url(base_url: &str, provider_kind: ProviderKind) -> String {
    let trimmed = base_url.trim_end_matches('/');
    match provider_kind {
        ProviderKind::OpenAiCompat => format!("{trimmed}/models"),
        ProviderKind::Anthropic => {
            if trimmed.ends_with("/v1") {
                format!("{trimmed}/models")
            } else {
                format!("{trimmed}/v1/models")
            }
        }
    }
}

#[derive(Debug, Clone)]
struct HealthProbeResult {
    status: String,
    latency_ms: Option<u64>,
    message: Option<String>,
}

impl HealthProbeResult {
    fn misconfigured(message: impl Into<String>) -> Self {
        Self {
            status: "misconfigured".to_string(),
            latency_ms: None,
            message: Some(message.into()),
        }
    }

    fn auth_failed(message: impl Into<String>) -> Self {
        Self {
            status: "auth_failed".to_string(),
            latency_ms: None,
            message: Some(message.into()),
        }
    }

    fn unreachable(message: impl Into<String>) -> Self {
        Self {
            status: "unreachable".to_string(),
            latency_ms: None,
            message: Some(message.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_probe_url, infer_provider_kind, ProviderKind};

    #[test]
    fn infer_provider_kind_uses_base_url() {
        assert_eq!(
            infer_provider_kind("https://api.anthropic.com"),
            ProviderKind::Anthropic
        );
        assert_eq!(
            infer_provider_kind("https://example.com/v1"),
            ProviderKind::OpenAiCompat
        );
    }

    #[test]
    fn build_probe_url_matches_provider_shape() {
        assert_eq!(
            build_probe_url("https://example.com/v1/", ProviderKind::OpenAiCompat),
            "https://example.com/v1/models"
        );
        assert_eq!(
            build_probe_url("https://api.anthropic.com", ProviderKind::Anthropic),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            build_probe_url("https://api.anthropic.com/v1", ProviderKind::Anthropic),
            "https://api.anthropic.com/v1/models"
        );
    }
}
