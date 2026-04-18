use crate::gateway::protocol::{ErrorPayload, GatewayMessage, MessageEnvelope};
use tokio::sync::mpsc;

pub fn send_general_error(
    outbound_tx: &mpsc::UnboundedSender<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: Option<String>,
) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.to_string(),
        MessageEnvelope::Error(ErrorPayload {
            message: error_msg,
            code,
        }),
    ));
}

/// 直接用于 handle_message 内部，不需要所有权转换的简化版本
pub fn send_general_error_direct(
    outbound_tx: &mpsc::UnboundedSender<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: Option<String>,
) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.to_string(),
        MessageEnvelope::Error(ErrorPayload {
            message: error_msg,
            code,
        }),
    ));
}
