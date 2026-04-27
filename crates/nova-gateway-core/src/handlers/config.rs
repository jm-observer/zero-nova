use crate::transport::ResponseSink;
use log::{error, info};
use nova_agent::AgentApplication;
use nova_protocol::{GatewayMessage, MessageEnvelope, SuccessResponse};
use serde_json::Value;

pub async fn handle_config_get(
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.config_snapshot() {
        Ok(config) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::ConfigGetResponse(config),
                ))
                .await;
        }
        Err(e) => {
            error!("Failed to serialize config snapshot: {}", e);
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some("SERVICE_ERROR".to_string()),
            )
            .await;
        }
    }
}

pub async fn handle_config_update(
    payload: Value,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Handling config update: {:?}", payload);

    match app.update_config(payload).await {
        Ok(()) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::ConfigUpdateResponse(SuccessResponse { success: true }),
                ))
                .await;
        }
        Err(e) => {
            error!("Failed to update config: {}", e);
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some(config_error_code(&e).to_string()),
            )
            .await;
        }
    }
}

fn config_error_code(error: &anyhow::Error) -> &'static str {
    if error.to_string().contains("parse config update payload") {
        "INVALID_REQUEST"
    } else {
        "SERVICE_ERROR"
    }
}
