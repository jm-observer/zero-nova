use super::{SttProvider, SynthesizeResult, TranscribeResult, TtsProvider};
use anyhow::Result;
use async_trait::async_trait;

pub struct MockSttProvider;

#[async_trait]
impl SttProvider for MockSttProvider {
    async fn transcribe(&self, _audio: &[u8], _format: &str, _language: Option<&str>) -> Result<TranscribeResult> {
        Ok(TranscribeResult {
            text: "mock transcription".to_string(),
            confidence: Some(1.0),
            duration_ms: None,
            segments: Vec::new(),
        })
    }
}

pub struct MockTtsProvider;

#[async_trait]
impl TtsProvider for MockTtsProvider {
    async fn synthesize(&self, _text: &str, _voice: Option<&str>) -> Result<SynthesizeResult> {
        Ok(SynthesizeResult {
            audio: vec![0_u8; 16],
            audio_format: "mp3".to_string(),
        })
    }
}
