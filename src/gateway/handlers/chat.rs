use crate::app::application::GatewayApplication;
use crate::gateway::bridge::app_event_to_gateway;
use crate::gateway::protocol::{ChatCompletePayload, ChatPayload, GatewayMessage, MessageEnvelope, SessionIdPayload};
use channel_websocket::ResponseSink;
use tokio::sync::mpsc;

pub async fn handle_chat(
    payload: ChatPayload,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let session_id = match payload.session_id {
        Some(id) => id,
        None => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                "session_id is required".to_string(),
                Some("INVALID_REQUEST".to_string()),
            );
            return;
        }
    };

    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session_id.clone();

    // 适配器：将应用层事件桥接到渠道层协议
    let event_forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg = app_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    match app.session_exists(&session_id).await {
        Ok(true) => {}
        Ok(false) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Session {} not found", session_id),
                Some("SESSION_NOT_FOUND".to_string()),
            );
            return;
        }
        Err(e) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some("SERVICE_ERROR".to_string()),
            );
            return;
        }
    }

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.clone(),
        MessageEnvelope::ChatStart(SessionIdPayload {
            session_id: session_id.clone(),
        }),
    ));

    if let Err(e) = app.start_turn(&session_id, &payload.input, event_tx).await {
        if let Err(join_error) = event_forwarder.await {
            log::error!(
                "Failed to join app event forwarder after start_turn error: {}",
                join_error
            );
        }
        crate::gateway::handlers::system::send_general_error(
            &outbound_tx,
            &request_id,
            format!("Service error: {}", e),
            Some(error_code(&e).to_string()),
        );
        return;
    }

    // 等到所有 progress 事件转发完成后再发 complete，避免前端看到乱序消息。
    if let Err(join_error) = event_forwarder.await {
        log::error!("Failed to join app event forwarder: {}", join_error);
    }

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::ChatComplete(ChatCompletePayload {
            session_id,
            output: None,
            usage: None,
        }),
    ));
}

pub async fn handle_chat_stop(
    payload: SessionIdPayload,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.stop_turn(&payload.session_id).await {
        Ok(()) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ChatStopResponse(payload),
            ));
        }
        Err(e) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some(error_code(&e).to_string()),
            );
        }
    }
}

fn error_code(error: &anyhow::Error) -> &'static str {
    if error.to_string().contains("Session not found") {
        "SESSION_NOT_FOUND"
    } else {
        "SERVICE_ERROR"
    }
}
