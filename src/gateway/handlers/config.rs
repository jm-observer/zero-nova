use crate::gateway::protocol::{GatewayMessage, MessageEnvelope, SuccessResponse};
use crate::gateway::router::AppState;
use log::{error, info};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_config_get<C: crate::provider::LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let config = state.config.read().unwrap();
    let val = serde_json::to_value(&*config).unwrap_or(Value::Null);

    let _ = outbound_tx.send(GatewayMessage::new(request_id, MessageEnvelope::ConfigGetResponse(val)));
}

pub async fn handle_config_update<C: crate::provider::LlmClient>(
    payload: Value,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    info!("Handling config update: {:?}", payload);

    // 1. Update in-memory config
    {
        let mut config = state.config.write().unwrap();
        // Simple merge or replacement (assuming full config for now)
        if let Ok(new_config) = serde_json::from_value::<crate::config::AppConfig>(payload) {
            *config = new_config;

            // 2. Save to file
            let config_str = toml::to_string(&*config).unwrap_or_default();
            if let Err(e) = std::fs::write(&state.config_path, config_str) {
                error!("Failed to save config to {:?}: {}", state.config_path, e);
            } else {
                info!("Config saved successfully to {:?}", state.config_path);
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
