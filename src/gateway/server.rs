use crate::app::application::GatewayApplication;
use crate::gateway::protocol::{GatewayMessage, MessageEnvelope, WelcomePayload};
use crate::gateway::router::handle_message;
use crate::provider::LlmClient;
use async_trait::async_trait;
use channel_websocket::{ChannelHandler, ResponseSink};
use std::net::SocketAddr;
use std::sync::Arc;

pub struct GatewayHandler<C: LlmClient> {
    app: Arc<GatewayApplication<C>>,
}

impl<C: LlmClient> GatewayHandler<C> {
    pub fn new(app: Arc<GatewayApplication<C>>) -> Self {
        Self { app }
    }
}

#[async_trait]
impl<C: LlmClient + 'static> ChannelHandler for GatewayHandler<C> {
    type Req = GatewayMessage;
    type Resp = GatewayMessage;

    async fn on_connect(&self, _peer: SocketAddr) -> anyhow::Result<Vec<Self::Resp>> {
        // 返回 Welcome 消息
        Ok(vec![GatewayMessage::new_event(MessageEnvelope::Welcome(
            WelcomePayload {
                require_auth: false,
                setup_required: false,
            },
        ))])
    }

    async fn on_message(
        &self,
        _peer: SocketAddr,
        req: Self::Req,
        response_sink: ResponseSink<Self::Resp>,
    ) -> anyhow::Result<()> {
        let app_clone = self.app.clone();
        tokio::spawn(async move {
            handle_message::<C>(req, app_clone, response_sink).await;
        });
        Ok(())
    }

    async fn on_disconnect(&self, peer: SocketAddr) {
        log::info!("Gateway peer disconnected: {}", peer);
    }
}

pub async fn run_server<C: LlmClient + 'static>(
    addr: SocketAddr,
    app: Arc<GatewayApplication<C>>,
) -> anyhow::Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    channel_websocket::run_server(addr, handler).await
}
