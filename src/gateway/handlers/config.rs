use crate::app::application::GatewayApplication;
use crate::gateway::protocol::{GatewayMessage, MessageEnvelope, SuccessResponse};
use channel_websocket::ResponseSink;
use log::{error, info};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_config_get<C: crate::provider::LlmClient>(
    app: Arc<GatewayApplication<C>>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let config = app.config.read().unwrap();
    let val = serde_json::to_value(&*config).unwrap_or(Value::Null);

    let _ = outbound_tx.send(GatewayMessage::new(request_id, MessageEnvelope::ConfigGetResponse(val)));
}

pub async fn handle_config_update<C: crate::provider::LlmClient>(
    payload: Value,
    app: Arc<GatewayApplication<C>>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Handling config update: {:?}", payload);

    // 1. Update in-memory config
    {
        let mut config = app.config.write().unwrap();
        // Simple merge or replacement (assuming full config for now)
        if let Ok(new_config) = serde_json::from_value::<crate::config::AppConfig>(payload) {
            *config = new_config;

            // 2. Save to file
            let config_str = toml::to_string(&*config).unwrap_or_default();
            if let Err(e) = std::fs::write(&app.config_path, config_str) {
                error!("Failed to save config to {:?}: {}", app.config_path, e);
            } else {
                info!("Config saved successfully to {:?}", app.config_path);
            }
        } else {
            error!("Failed to parse config update payload");
        }
    }

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::ConfigUpdateResponse(SuccessResponse { success: true }),
    ));
}
