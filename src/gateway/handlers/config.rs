use crate::app::application::GatewayApplication;
use crate::gateway::protocol::{GatewayMessage, MessageEnvelope, SuccessResponse};
use anyhow::Result;
use channel_websocket::ResponseSink;
use log::{error, info};
use serde_json::Value;

pub async fn handle_config_get<C: crate::provider::LlmClient + 'static>(
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.config_snapshot() {
        Ok(config) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ConfigGetResponse(config),
            ));
        }
        Err(e) => {
            error!("Failed to serialize config snapshot: {}", e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some("SERVICE_ERROR".to_string()),
            );
        }
    }
}

pub async fn handle_config_update<C: crate::provider::LlmClient + 'static>(
    payload: Value,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Handling config update: {:?}", payload);

    match update_config(app, payload).await {
        Ok(()) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ConfigUpdateResponse(SuccessResponse { success: true }),
            ));
        }
        Err(e) => {
            error!("Failed to update config: {}", e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Service error: {}", e),
                Some(config_error_code(&e).to_string()),
            );
        }
    }
}

async fn update_config<C: crate::provider::LlmClient + 'static>(
    app: &GatewayApplication<C>,
    payload: Value,
) -> Result<()> {
    app.update_config(payload).await
}

fn config_error_code(error: &anyhow::Error) -> &'static str {
    if error.to_string().contains("parse config update payload") {
        "INVALID_REQUEST"
    } else {
        "SERVICE_ERROR"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_failures_map_to_invalid_request() {
        let err = anyhow::anyhow!("Failed to parse config update payload");
        assert_eq!(config_error_code(&err), "INVALID_REQUEST");
    }

    #[test]
    fn other_failures_map_to_service_error() {
        let err = anyhow::anyhow!("{}", serde_json::json!({ "write": "failed" }));
        assert_eq!(config_error_code(&err), "SERVICE_ERROR");
    }
}
