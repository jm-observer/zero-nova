use anyhow::Result;
use async_trait::async_trait;
use nova_protocol::voice::VoiceSegment;

pub mod mock;
pub mod openai_compat;

#[derive(Debug, Clone)]
pub struct TranscribeResult {
    pub text: String,
    pub confidence: Option<f32>,
    pub duration_ms: Option<u64>,
    pub segments: Vec<VoiceSegment>,
}

#[derive(Debug, Clone)]
pub struct SynthesizeResult {
    pub audio: Vec<u8>,
    pub audio_format: String,
}

#[async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(&self, audio: &[u8], format: &str, language: Option<&str>) -> Result<TranscribeResult>;
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<SynthesizeResult>;
}
