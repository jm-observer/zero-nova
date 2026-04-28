use super::{SttProvider, SynthesizeResult, TranscribeResult, TtsProvider};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;

pub struct OpenAiCompatSttProvider {
    http: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiCompatSttProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url,
            model,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SttResponse {
    text: String,
}

#[async_trait]
impl SttProvider for OpenAiCompatSttProvider {
    async fn transcribe(&self, audio: &[u8], format: &str, language: Option<&str>) -> Result<TranscribeResult> {
        let url = format!("{}/audio/transcriptions", self.base_url);
        let body = json!({
            "model": self.model,
            "audio_base64": crate::app::voice_service::encode_base64(audio),
            "audio_format": format,
            "language": language,
        });

        let response = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to send stt request")?
            .error_for_status()
            .context("stt provider returned error status")?;

        let payload: SttResponse = response.json().await.context("failed to decode stt response")?;
        Ok(TranscribeResult {
            text: payload.text,
            confidence: None,
            duration_ms: None,
            segments: Vec::new(),
        })
    }
}

pub struct OpenAiCompatTtsProvider {
    http: Client,
    api_key: String,
    base_url: String,
    model: String,
    default_voice: String,
}

impl OpenAiCompatTtsProvider {
    pub fn new(api_key: String, base_url: String, model: String, default_voice: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url,
            model,
            default_voice,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TtsResponse {
    audio_base64: String,
    #[serde(default)]
    audio_format: Option<String>,
}

#[async_trait]
impl TtsProvider for OpenAiCompatTtsProvider {
    async fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<SynthesizeResult> {
        let url = format!("{}/audio/speech", self.base_url);
        let body = json!({
            "model": self.model,
            "input": text,
            "voice": voice.unwrap_or(&self.default_voice),
            "format": "mp3"
        });

        let response = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to send tts request")?
            .error_for_status()
            .context("tts provider returned error status")?;

        let payload: TtsResponse = response.json().await.context("failed to decode tts response")?;
        let audio = crate::app::voice_service::decode_base64(&payload.audio_base64)
            .context("failed to decode tts audio base64")?;
        Ok(SynthesizeResult {
            audio,
            audio_format: payload.audio_format.unwrap_or_else(|| "mp3".to_string()),
        })
    }
}
