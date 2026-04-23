use crate::app::application::GatewayApplication;
use crate::gateway::protocol::GatewayMessage;
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
        self.app.connect().await
    }

    async fn on_message(
        &self,
        _peer: SocketAddr,
        req: Self::Req,
        response_sink: ResponseSink<Self::Resp>,
    ) -> anyhow::Result<()> {
        let app_clone = self.app.clone();
        tokio::spawn(async move {
            app_clone.handle(req, response_sink).await;
        });
        Ok(())
    }

    async fn on_disconnect(&self, peer: SocketAddr) {
        self.app.disconnect(peer).await;
    }
}

pub async fn run_server<C: LlmClient + 'static>(
    addr: SocketAddr,
    app: Arc<GatewayApplication<C>>,
) -> anyhow::Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    channel_websocket::run_server(addr, handler).await
}
