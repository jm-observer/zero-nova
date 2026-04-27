use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc;

pub type PeerId = String;

#[async_trait]
pub trait ChannelHandler: Send + Sync + 'static {
    type Req: DeserializeOwned + Send + 'static;
    type Resp: Serialize + Send + 'static;

    async fn on_connect(&self, peer: PeerId) -> anyhow::Result<Vec<Self::Resp>>;
    async fn on_message(&self, peer: PeerId, req: Self::Req, sink: ResponseSink<Self::Resp>) -> anyhow::Result<()>;
    async fn on_disconnect(&self, peer: PeerId);
}

#[derive(Debug)]
pub struct ResponseSink<R> {
    tx: mpsc::Sender<R>,
}

impl<R> ResponseSink<R> {
    pub fn new(tx: mpsc::Sender<R>) -> Self {
        Self { tx }
    }

    pub fn send(&self, msg: R) -> Result<(), mpsc::error::TrySendError<R>> {
        self.tx.try_send(msg)
    }

    pub async fn send_async(&self, msg: R) -> Result<(), mpsc::error::SendError<R>> {
        self.tx.send(msg).await
    }
}

impl<R> Clone for ResponseSink<R> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}
