use crate::{ChannelHandler, ResponseSink};
use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_stdio<H, Req, Resp>(handler: Arc<H>) -> Result<()>
where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    let peer_id = "stdio".to_string();

    // On connect
    let initial_messages = handler.on_connect(peer_id.clone()).await?;
    for msg in initial_messages {
        let json = serde_json::to_string(&msg)?;
        let mut out = stdout();
        out.write_all(json.as_bytes()).await?;
        out.write_all(b"\n").await?;
        out.flush().await?;
    }

    let (sink_tx, mut sink_rx) = tokio::sync::mpsc::channel::<Resp>(100);
    let sink = ResponseSink::new(sink_tx);

    // Stdout writer task
    tokio::spawn(async move {
        let mut out = stdout();
        while let Some(msg) = sink_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    let _ = out.write_all(json.as_bytes()).await;
                    let _ = out.write_all(b"\n").await;
                    let _ = out.flush().await;
                }
                Err(e) => {
                    log::error!("Failed to serialize message: {}", e);
                }
            }
        }
    });

    // Stdin reader loop
    let mut reader = BufReader::new(stdin()).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        match serde_json::from_str::<Req>(&line) {
            Ok(req) => {
                if let Err(e) = handler.on_message(peer_id.clone(), req, sink.clone()).await {
                    log::error!("Error handling message: {}", e);
                }
            }
            Err(e) => {
                log::error!("Invalid JSON on stdin: {} | Content: {}", e, line);
            }
        }
    }

    handler.on_disconnect(peer_id).await;
    Ok(())
}
