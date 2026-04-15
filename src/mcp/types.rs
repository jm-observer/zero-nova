// use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON‑RPC 2.0 request
#[derive(Debug, Serialize, Clone)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str, // "2.0"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON‑RPC 2.0 response
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// Result struct for the `initialize` method.
#[derive(Debug, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// Server capabilities returned by the server.
#[derive(Debug, Deserialize, Default)]
/// Represents the server's declared capabilities.
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<ToolsCapability>,
    #[serde(default)]
    pub resources: Option<Value>,
    #[serde(default)]
    pub prompts: Option<Value>,
}

#[derive(Debug, Deserialize)]
/// Capability indicating available tools.
pub struct ToolsCapability {}

#[derive(Debug, Deserialize)]
/// Information about the MCP server.
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP tool definition (returned by tools/list)
#[derive(Debug, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// tools/list result
#[derive(Debug, Deserialize)]
/// Result of the `tools/list` RPC call.
pub struct ListToolsResult {
    pub tools: Vec<McpToolDef>,
}

/// tools/call result
#[derive(Debug, Deserialize)]
/// Result of the `tools/call` RPC call.
pub struct CallToolResult {
    pub content: Vec<McpContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: Value },
}
