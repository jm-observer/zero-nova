use crate::transport::ResponseSink;
use nova_protocol::{ErrorPayload, GatewayMessage, MessageEnvelope};

pub async fn send_general_error(
    outbound_tx: &ResponseSink<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: Option<String>,
) {
    let _ = outbound_tx
        .send_async(GatewayMessage::new(
            request_id.to_string(),
            MessageEnvelope::Error(ErrorPayload {
                message: error_msg,
                code,
            }),
        ))
        .await;
}

/// 直接用于 handle_message 内部，不需要所有权转换的简化版本
pub async fn send_general_error_direct(
    outbound_tx: &ResponseSink<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: Option<String>,
) {
    send_general_error(outbound_tx, request_id, error_msg, code).await;
}
