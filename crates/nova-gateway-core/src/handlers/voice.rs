use channel_core::ResponseSink;
use nova_agent::app::AgentApplication;
use nova_protocol::voice::{VoiceCapability, VoiceErrorCode, VoiceErrorPayload};
use nova_protocol::{GatewayMessage, MessageEnvelope, VoiceTranscribeRequest, VoiceTtsRequest};

pub async fn handle_voice_capabilities(
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.voice_capabilities().await {
        Ok(payload) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::VoiceCapabilitiesResponse(payload),
                ))
                .await;
        }
        Err(err) => {
            send_voice_error(
                &outbound_tx,
                request_id,
                VoiceErrorCode::VoiceSttUnavailable,
                VoiceCapability::Transport,
                err.to_string(),
                None,
            )
            .await;
        }
    }
}

pub async fn handle_voice_transcribe(
    payload: VoiceTranscribeRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let session_id = payload.session_id.clone();
    match app.voice_transcribe(&payload).await {
        Ok(response) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::VoiceTranscribeResponse(response),
                ))
                .await;
        }
        Err(err) => {
            let (code, message) = map_stt_error(&err.to_string());
            send_voice_error(
                &outbound_tx,
                request_id,
                code,
                VoiceCapability::Stt,
                message,
                session_id,
            )
            .await;
        }
    }
}

pub async fn handle_voice_tts(
    payload: VoiceTtsRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let session_id = payload.session_id.clone();
    match app.voice_tts(&payload).await {
        Ok(response) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::VoiceTtsResponse(response),
                ))
                .await;
        }
        Err(err) => {
            let (code, message) = map_tts_error(&err.to_string());
            send_voice_error(
                &outbound_tx,
                request_id,
                code,
                VoiceCapability::Tts,
                message,
                session_id,
            )
            .await;
        }
    }
}

fn map_stt_error(error: &str) -> (VoiceErrorCode, String) {
    if error.contains("voice_stt_timeout") {
        (VoiceErrorCode::VoiceSttTimeout, "stt timed out".to_string())
    } else if error.contains("too large") {
        (VoiceErrorCode::VoiceInputTooShort, "audio input too large".to_string())
    } else if error.contains("invalid audio base64") {
        (VoiceErrorCode::VoiceDecodeFailed, "invalid audio encoding".to_string())
    } else if error.contains("voice_request_in_progress") {
        (
            VoiceErrorCode::VoiceRequestInProgress,
            "voice_request_in_progress".to_string(),
        )
    } else {
        (VoiceErrorCode::VoiceSttUnavailable, "stt unavailable".to_string())
    }
}

fn map_tts_error(error: &str) -> (VoiceErrorCode, String) {
    if error.contains("voice_tts_timeout") {
        (VoiceErrorCode::VoiceTtsTimeout, "tts timed out".to_string())
    } else {
        (VoiceErrorCode::VoiceTtsUnavailable, "tts unavailable".to_string())
    }
}

async fn send_voice_error(
    outbound_tx: &ResponseSink<GatewayMessage>,
    request_id: String,
    code: VoiceErrorCode,
    capability: VoiceCapability,
    message: String,
    session_id: Option<String>,
) {
    let _ = outbound_tx
        .send_async(GatewayMessage::new(
            request_id.clone(),
            MessageEnvelope::VoiceError(VoiceErrorPayload {
                code,
                message,
                capability,
                request_id: Some(request_id),
                session_id,
                turn_id: None,
            }),
        ))
        .await;
}
