pub mod bridge;
pub mod handlers;
pub mod push_center;
pub mod router;

pub use bridge::{app_agent_to_protocol, app_event_to_gateway, app_message_to_protocol, app_session_to_protocol};
pub use push_center::PushCenter;
pub use router::dispatch;

use anyhow::Result;
use async_trait::async_trait;
use channel_core::{ChannelHandler, PeerId, ResponseSink};
use log::trace;
use nova_agent::app::AgentApplicationImpl;
use nova_protocol::GatewayMessage;
use std::sync::Arc;

pub struct GatewayHandler {
    app: Arc<AgentApplicationImpl>,
    push_center: Arc<PushCenter>,
}

impl GatewayHandler {
    pub fn new(app: Arc<AgentApplicationImpl>) -> Self {
        Self {
            app,
            push_center: Arc::new(PushCenter::new()),
        }
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

    async fn on_message(&self, peer: PeerId, req: Self::Req, sink: ResponseSink<Self::Resp>) -> Result<()> {
        trace!("[INBOUND] GatewayHandler::on_message: {:?}", req);
        self.push_center.register_peer(peer.clone(), sink.clone()).await;
        dispatch(req, &peer, &self.app, sink, self.push_center.clone()).await;
        Ok(())
    }

    async fn on_disconnect(&self, peer: PeerId) {
        self.push_center.unregister_peer(&peer).await;
        self.app.on_disconnect(&peer).await;
    }
}
