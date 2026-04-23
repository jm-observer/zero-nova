// src/mcp/transport.rs
use crate::mcp::types::{JsonRpcRequest, JsonRpcResponse};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};

#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Sends a JSON-RPC request and awaits the response.
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse>;
    /// Sends a JSON-RPC notification (no response expected).
    async fn notify(&self, request: JsonRpcRequest) -> Result<()>;
    /// Closes the transport, cleaning up resources.
    async fn close(&self) -> Result<()>;
}

type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>;

/// Stdio transport – spawns a child process and talks JSON‑RPC over its stdin/stdout.
/// Transport implementation using a child process via stdio.
pub struct StdioTransport {
    child: Mutex<Child>,
    stdin: Mutex<BufWriter<tokio::process::ChildStdin>>,
    pending: PendingRequests,
}

impl StdioTransport {
    /// Spawn a command (e.g., `npx -y @modelcontextprotocol/server-filesystem /tmp`)
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to open child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to open child stdout"))?;

        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        // Background reader loop
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(line.trim_end()) {
                    if let Some(id) = resp.id {
                        let mut p = pending_clone.lock().await;
                        if let Some(tx) = p.remove(&id) {
                            let _ = tx.send(resp);
                        }
                    }
                }
                line.clear();
            }
        });

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(BufWriter::new(stdin)),
            pending,
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    /// Sends a JSON-RPC request and awaits the response.
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let id = request.id.ok_or_else(|| anyhow!("Request ID missing"))?;
        let (tx, rx) = oneshot::channel();

        {
            let mut p = self.pending.lock().await;
            p.insert(id, tx);
        }

        let json = serde_json::to_string(&request)? + "\n";
        {
            let mut writer = self.stdin.lock().await;
            writer.write_all(json.as_bytes()).await?;
            writer.flush().await?;
        }

        rx.await.map_err(|_| anyhow!("Response channel closed"))
    }

    /// Sends a JSON-RPC notification (no response expected).
    async fn notify(&self, request: JsonRpcRequest) -> Result<()> {
        let json = serde_json::to_string(&request)? + "\n";
        {
            let mut writer = self.stdin.lock().await;
            writer.write_all(json.as_bytes()).await?;
            writer.flush().await?;
        }
        Ok(())
    }

    /// Closes the transport, cleaning up resources.
    async fn close(&self) -> Result<()> {
        let mut child = self.child.lock().await;
        child.kill().await.ok();
        let _ = child.wait().await;
        Ok(())
    }
}

/// WebSocket transport – connects to a ws:// or wss:// endpoint.
/// Transport implementation using a WebSocket connection.
pub struct WebSocketTransport {
    write_sink: Mutex<futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>>,
    pending: PendingRequests,
}

impl WebSocketTransport {
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws_stream, _): (WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, _) = connect_async(url).await?;
        let (write_sink, mut read_stream) = ws_stream.split();
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        // Background reader loop
        tokio::spawn(async move {
            while let Some(msg) = read_stream.next().await {
                if let Ok(Message::Text(txt)) = msg {
                    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&txt) {
                        if let Some(id) = resp.id {
                            let mut p = pending_clone.lock().await;
                            if let Some(tx) = p.remove(&id) {
                                let _ = tx.send(resp);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            write_sink: Mutex::new(write_sink),
            pending,
        })
    }
}

#[async_trait]
impl McpTransport for WebSocketTransport {
    /// Sends a JSON-RPC request and awaits the response.
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let id = request.id.ok_or_else(|| anyhow!("Request ID missing"))?;
        let (tx, rx) = oneshot::channel();

        {
            let mut p = self.pending.lock().await;
            p.insert(id, tx);
        }

        let json = serde_json::to_string(&request)?;
        {
            let mut sink = self.write_sink.lock().await;
            let _ = sink.send(Message::Text(json)).await;
        }

        rx.await.map_err(|_| anyhow!("Response channel closed"))
    }

    /// Sends a JSON-RPC notification (no response expected).
    async fn notify(&self, request: JsonRpcRequest) -> Result<()> {
        let json = serde_json::to_string(&request)?;
        {
            let mut sink = self.write_sink.lock().await;
            let _ = sink.send(Message::Text(json)).await;
        }
        Ok(())
    }

    /// Closes the transport, cleaning up resources.
    async fn close(&self) -> Result<()> {
        let mut sink = self.write_sink.lock().await;
        sink.close().await?;
        Ok(())
    }
}
