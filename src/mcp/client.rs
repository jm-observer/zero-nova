// src/mcp/client.rs
use anyhow::{Result, anyhow};
// use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
// use std::sync::Arc;

use crate::mcp::transport::{McpTransport, StdioTransport, WebSocketTransport};
use crate::mcp::types::{
    CallToolResult, InitializeResult, JsonRpcRequest, ListToolsResult, McpToolDef, ServerCapabilities, ServerInfo,
};

/// Client for communicating with an MCP server.
pub struct McpClient {
    transport: Box<dyn McpTransport>,
    server_info: Option<ServerInfo>,
    capabilities: Option<ServerCapabilities>,
}

impl McpClient {
    /// Connect via stdio (spawn a child process)
    pub async fn connect_stdio(command: &str, args: &[&str]) -> Result<Self> {
        let transport = StdioTransport::spawn(command, args).await?;
        let mut client = Self {
            transport: Box::new(transport),
            server_info: None,
            capabilities: None,
        };
        client.initialize().await?;
        Ok(client)
    }

    /// Connect via WebSocket
    pub async fn connect_ws(url: &str) -> Result<Self> {
        let transport = WebSocketTransport::connect(url).await?;
        let mut client = Self {
            transport: Box::new(transport),
            server_info: None,
            capabilities: None,
        };
        client.initialize().await?;
        Ok(client)
    }

    /// Perform the "initialize" handshake and send the "initialized" notification.
    async fn initialize(&mut self) -> Result<()> {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: Some(self.next_id()),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "zero-nova",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };
        let resp = self.transport.send(req).await?;
        let result: InitializeResult = match resp.result {
            Some(val) => serde_json::from_value(val)?,
            None => return Err(anyhow!("initialize response missing result: {:?}", resp.error)),
        };
        self.server_info = Some(result.server_info);
        self.capabilities = Some(result.capabilities);

        // send initialized notification (no id needed in standard JSON-RPC)
        let notif = JsonRpcRequest {
            jsonrpc: "2.0",
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        self.transport.notify(notif).await?;
        Ok(())
    }

    /// List available tools from server
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let result: ListToolsResult = self.call("tools/list", json!({})).await?;
        Ok(result.tools)
    }

    /// Call a specific tool on the server
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult> {
        self.call("tools/call", json!({"name": name, "arguments": arguments}))
            .await
    }

    /// Return server info (if initialized)
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Close the underlying transport
    pub async fn close(self) -> Result<()> {
        self.transport.close().await
    }

    // ---------- internal helpers ----------
    /// Generates a new unique request ID.
    fn next_id(&self) -> u64 {
        // Simple wrapper – generate a fresh id each time; use atomic internally via transport if needed.
        // For simplicity we just use a random high number (client side doesn't need to be atomic across calls)
        // but to keep deterministic for testing we use a static counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Sends a JSON-RPC request and deserializes the response.
    async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: Some(self.next_id()),
            method: method.to_string(),
            params: Some(params),
        };
        let resp = self.transport.send(request).await?;
        match resp.result {
            Some(val) => {
                let parsed: T = serde_json::from_value(val)?;
                Ok(parsed)
            }
            None => {
                let err_msg = resp
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "Unknown RPC error".to_string());
                Err(anyhow!("RPC call '{}' failed: {}", method, err_msg))
            }
        }
    }

    // notify method is unused; notifications are sent directly in `initialize`.
}
