pub mod transport;

use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;

use transport::core::ChannelHandler;

pub async fn run_stdio<H, Req, Resp>(handler: Arc<H>) -> Result<()>
where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    transport::stdio::run_stdio(handler).await
}

pub async fn run_server<H, Req, Resp>(addr: &str, handler: Arc<H>) -> Result<()>
where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    transport::ws::run_server(addr, handler).await
}
