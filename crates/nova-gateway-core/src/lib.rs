pub mod bridge;
pub mod handlers;
pub mod router;

pub use bridge::{app_agent_to_protocol, app_event_to_gateway, app_message_to_protocol, app_session_to_protocol};
pub use router::dispatch;

use anyhow::Result;
use async_trait::async_trait;
use channel_core::{ChannelHandler, PeerId, ResponseSink};
use log::debug;
use nova_agent::app::AgentApplication;
use nova_protocol::GatewayMessage;
use std::sync::Arc;

pub struct GatewayHandler {
    app: Arc<dyn AgentApplication>,
}

impl GatewayHandler {
    pub fn new(app: Arc<dyn AgentApplication>) -> Self {
        Self { app }
    }
}

#[async_trait]
impl ChannelHandler for GatewayHandler {
    type Req = GatewayMessage;
    type Resp = GatewayMessage;

    async fn on_connect(&self, _peer: PeerId) -> Result<Vec<Self::Resp>> {
        let events = self.app.on_connect().await?;
        let mut responses = Vec::new();
        for event in events {
            responses.push(app_event_to_gateway(event, "0", "0"));
        }
        Ok(responses)
    }

    async fn on_message(&self, _peer: PeerId, req: Self::Req, sink: ResponseSink<Self::Resp>) -> Result<()> {
        debug!("[INBOUND] GatewayHandler::on_message: {:?}", req);
        dispatch(req, &*self.app, sink).await;
        Ok(())
    }

    async fn on_disconnect(&self, peer: PeerId) {
        self.app.on_disconnect(&peer).await;
    }
}
