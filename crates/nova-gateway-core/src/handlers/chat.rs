use crate::bridge::app_event_to_gateway;
use crate::handlers::system::send_general_error;
use crate::PushCenter;
use channel_core::ResponseSink;
use log::{debug, trace};
use nova_agent::app::AgentApplication;
use nova_protocol::{ChatCompletePayload, ChatPayload, GatewayMessage, MessageEnvelope, SessionIdPayload, Usage};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_chat(
    payload: ChatPayload,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
    peer_id: &str,
    push_center: Arc<PushCenter>,
) {
    let session_id: String = match payload.session_id {
        Some(id) => id,
        None => {
            send_general_error(
                &outbound_tx,
                &request_id,
                "session_id is required".to_string(),
                Some("INVALID_REQUEST".to_string()),
            )
            .await;
            return;
        }
    };

    push_center.subscribe_peer_to_session(peer_id, &session_id).await;

    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let push_center_clone = push_center.clone();
    let peer_id_owned = peer_id.to_string();
    let session_id_clone = session_id.clone();

    // 当前请求连接保持直连，其余订阅同 session 的连接走广播，避免跨会话污染和重复 complete。
    let event_forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            trace!(
                "[OUTBOUND] Event forwarder: broadcasting event type={:?} to session={}",
                std::any::type_name_of_val(&event),
                session_id_clone
            );
            let gateway_msg = app_event_to_gateway(event, "", &session_id_clone);
            if outbound_tx_clone.send_async(gateway_msg.clone()).await.is_err() {
                break;
            }
            push_center_clone
                .broadcast_to_session_except(&session_id_clone, Some(&peer_id_owned), gateway_msg)
                .await;
        }
    });

    match app.session_exists(&session_id).await {
        Ok(true) => {}
        Ok(false) => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Session {} not found", session_id),
                Some("SESSION_NOT_FOUND".to_string()),
            )
            .await;
            return;
        }
        Err(e) => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some("SERVICE_ERROR".to_string()),
            )
            .await;
            return;
        }
    }

    let _ = outbound_tx
        .send_async(GatewayMessage::new(
            request_id.clone(),
            MessageEnvelope::ChatStart(SessionIdPayload {
                session_id: session_id.clone(),
            }),
        ))
        .await;

    debug!(
        "[OUTBOUND] Calling start_turn for session={}, input_len={}",
        session_id,
        payload.input.len()
    );
    let turn_result = match app.start_turn(&session_id, &payload.input, event_tx).await {
        Ok(res) => res,
        Err(e) => {
            if let Err(join_error) = event_forwarder.await {
                log::error!(
                    "Failed to join app event forwarder after start_turn error: {}",
                    join_error
                );
            }
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some(error_code(&e).to_string()),
            )
            .await;
            return;
        }
    };

    // 等到所有 progress 事件转发完成后再发 complete，避免前端看到乱序消息。
    if let Err(join_error) = event_forwarder.await {
        log::error!("Failed to join app event forwarder: {}", join_error);
    }

    let complete_usage = Usage {
        input_tokens: turn_result.usage.input_tokens,
        output_tokens: turn_result.usage.output_tokens,
        cache_creation_input_tokens: turn_result.usage.cache_creation_input_tokens,
        cache_read_input_tokens: turn_result.usage.cache_read_input_tokens,
    };
    let broadcast_session_id = session_id.clone();

    let _ = outbound_tx
        .send_async(GatewayMessage::new(
            request_id,
            MessageEnvelope::ChatComplete(ChatCompletePayload {
                session_id,
                output: None,
                usage: Some(complete_usage.clone()),
            }),
        ))
        .await;

    push_center
        .broadcast_to_session_except(
            &broadcast_session_id,
            Some(peer_id),
            GatewayMessage::new_event(MessageEnvelope::ChatComplete(ChatCompletePayload {
                session_id: broadcast_session_id.clone(),
                output: None,
                usage: Some(complete_usage),
            })),
        )
        .await;
}

pub async fn handle_chat_stop(
    payload: SessionIdPayload,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.stop_turn(&payload.session_id).await {
        Ok(()) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::ChatStopResponse(payload),
                ))
                .await;
        }
        Err(e) => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some(error_code(&e).to_string()),
            )
            .await;
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
