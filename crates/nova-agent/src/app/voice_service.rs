use crate::config::VoiceConfig;
use crate::voice::{SttProvider, TtsProvider};
use anyhow::{anyhow, bail, Context, Result};
use nova_protocol::voice::{VoiceErrorCode, VoiceTranscribeResponse, VoiceTtsResponse};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

pub struct VoiceService {
    config: VoiceConfig,
    stt_provider: Arc<dyn SttProvider>,
    tts_provider: Arc<dyn TtsProvider>,
    transcribe_sessions: Arc<Mutex<HashSet<String>>>,
}

impl VoiceService {
    pub fn new(config: VoiceConfig, stt_provider: Arc<dyn SttProvider>, tts_provider: Arc<dyn TtsProvider>) -> Self {
        Self {
            config,
            stt_provider,
            tts_provider,
            transcribe_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn transcribe(
        &self,
        session_id: Option<&str>,
        audio_base64: &str,
        audio_format: &str,
        language: Option<&str>,
    ) -> Result<VoiceTranscribeResponse> {
        let sid = session_id.unwrap_or("global").to_string();
        self.guard_transcribe_session(&sid).await?;
        let result = self.transcribe_inner(audio_base64, audio_format, language).await;
        self.release_transcribe_session_later(sid);
        result
    }

    async fn transcribe_inner(
        &self,
        audio_base64: &str,
        audio_format: &str,
        language: Option<&str>,
    ) -> Result<VoiceTranscribeResponse> {
        if !self.config.enabled {
            bail!("voice is disabled");
        }
        let audio = decode_base64(audio_base64).context("invalid audio base64")?;
        if audio.is_empty() {
            bail!("empty audio input");
        }
        if audio.len() > self.config.max_input_bytes {
            bail!("audio input too large");
        }

        let transcribe = self.stt_provider.transcribe(&audio, audio_format, language);
        let result = timeout(Duration::from_millis(self.config.stt_timeout_ms), transcribe)
            .await
            .map_err(|_| anyhow!(VoiceErrorCode::VoiceSttTimeout.as_str()))??;

        Ok(VoiceTranscribeResponse {
            text: result.text,
            confidence: result.confidence,
            duration_ms: result.duration_ms,
            segments: result.segments,
        })
    }

    pub async fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<VoiceTtsResponse> {
        if !self.config.enabled {
            bail!("voice is disabled");
        }
        if text.trim().is_empty() {
            bail!("empty tts input");
        }

        let synthesize = self.tts_provider.synthesize(text, voice);
        let result = timeout(Duration::from_millis(self.config.tts_timeout_ms), synthesize)
            .await
            .map_err(|_| anyhow!(VoiceErrorCode::VoiceTtsTimeout.as_str()))??;
        Ok(VoiceTtsResponse {
            audio_format: result.audio_format,
            audio_base64: encode_base64(&result.audio),
        })
    }

    async fn guard_transcribe_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.transcribe_sessions.lock().await;
        if sessions.contains(session_id) {
            bail!(VoiceErrorCode::VoiceRequestInProgress.as_str());
        }
        sessions.insert(session_id.to_string());
        Ok(())
    }

    fn release_transcribe_session_later(&self, session_id: String) {
        let sessions = self.transcribe_sessions.clone();
        tokio::spawn(async move {
            let mut guard = sessions.lock().await;
            guard.remove(&session_id);
        });
    }
}

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(data: &[u8]) -> String {
    let mut output = String::new();
    let mut i = 0usize;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        output.push(BASE64_TABLE[((n >> 18) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[((n >> 12) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[((n >> 6) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }

    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        output.push(BASE64_TABLE[((n >> 18) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[((n >> 12) & 0x3f) as usize] as char);
        output.push('=');
        output.push('=');
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        output.push(BASE64_TABLE[((n >> 18) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[((n >> 12) & 0x3f) as usize] as char);
        output.push(BASE64_TABLE[((n >> 6) & 0x3f) as usize] as char);
        output.push('=');
    }

    output
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        bail!("invalid base64 length");
    }

    let mut output = Vec::with_capacity((bytes.len() / 4) * 3);
    for chunk in bytes.chunks(4) {
        let a = decode_base64_char(chunk[0])? as u32;
        let b = decode_base64_char(chunk[1])? as u32;
        let c = if chunk[2] == b'=' {
            0
        } else {
            decode_base64_char(chunk[2])? as u32
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            decode_base64_char(chunk[3])? as u32
        };
        let n = (a << 18) | (b << 12) | (c << 6) | d;

        output.push(((n >> 16) & 0xff) as u8);
        if chunk[2] != b'=' {
            output.push(((n >> 8) & 0xff) as u8);
        }
        if chunk[3] != b'=' {
            output.push((n & 0xff) as u8);
        }
    }
    Ok(output)
}

fn decode_base64_char(c: u8) -> Result<u8> {
    let idx = BASE64_TABLE
        .iter()
        .position(|v| *v == c)
        .ok_or_else(|| anyhow!("invalid base64 character"))?;
    Ok(idx as u8)
}

trait VoiceErrorCodeExt {
    fn as_str(&self) -> &'static str;
}

impl VoiceErrorCodeExt for VoiceErrorCode {
    fn as_str(&self) -> &'static str {
        match self {
            VoiceErrorCode::VoiceSttTimeout => "voice_stt_timeout",
            VoiceErrorCode::VoiceTtsTimeout => "voice_tts_timeout",
            VoiceErrorCode::VoiceInputTooShort => "voice_input_too_short",
            VoiceErrorCode::VoiceFormatUnsupported => "voice_format_unsupported",
            VoiceErrorCode::VoiceDecodeFailed => "voice_decode_failed",
            VoiceErrorCode::VoiceSttUnavailable => "voice_stt_unavailable",
            VoiceErrorCode::VoiceTtsUnavailable => "voice_tts_unavailable",
            VoiceErrorCode::VoiceRequestInProgress => "voice_request_in_progress",
        }
    }
}
