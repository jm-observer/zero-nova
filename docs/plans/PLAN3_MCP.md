# Plan 3: MCP 支持

## 目标

Agent 能连接外部 MCP server，发现并使用其工具，与内置工具统一调度。

## 前置

Plan 2 完成。

## 范围

| # | 文件 | 内容 |
|---|------|------|
| 1 | `src/mcp/types.rs` | MCP JSON-RPC 协议类型 |
| 2 | `src/mcp/transport.rs` | Transport trait + StdioTransport + WebSocketTransport |
| 3 | `src/mcp/client.rs` | McpClient（initialize、list_tools、call_tool） |
| 4 | `src/mcp/mod.rs` | MCP 子系统入口与公开 API |
| 5 | `src/tool/mcp.rs` | McpToolBridge — 将 MCP 工具适配为 Tool trait |

## 详细设计

### 1. mcp/types.rs — JSON-RPC 协议类型

MCP 基于 JSON-RPC 2.0，定义以下核心类型：

```rust
/// JSON-RPC 请求
#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,  // "2.0"
    pub id: u64,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 响应
#[derive(Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// MCP initialize 响应
#[derive(Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

#[derive(Deserialize)]
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<ToolsCapability>,
    #[serde(default)]
    pub resources: Option<serde_json::Value>,
    #[serde(default)]
    pub prompts: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct ToolsCapability {}

#[derive(Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP tool 定义（从 tools/list 返回）
#[derive(Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP tools/list 响应
#[derive(Deserialize)]
pub struct ListToolsResult {
    pub tools: Vec<McpToolDef>,
}

/// MCP tools/call 响应
#[derive(Deserialize)]
pub struct CallToolResult {
    pub content: Vec<McpContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: serde_json::Value },
}
```

### 2. mcp/transport.rs — 传输层抽象

```rust
/// MCP 传输层抽象
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// 发送 JSON-RPC 请求并等待响应
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse>;

    /// 关闭连接
    async fn close(&self) -> Result<()>;
}

/// Stdio 传输（启动子进程，通过 stdin/stdout 通信）
pub struct StdioTransport {
    child: tokio::process::Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    next_id: AtomicU64,
}

impl StdioTransport {
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self>;
}

/// WebSocket 传输
pub struct WebSocketTransport {
    ws: tokio_tungstenite::WebSocketStream<...>,
    next_id: AtomicU64,
}

impl WebSocketTransport {
    pub async fn connect(url: &str) -> Result<Self>;
}
```

**StdioTransport 通信协议**：
- 每条 JSON-RPC 消息以 `\n` 分隔（newline-delimited JSON）
- 写入 stdin → 从 stdout 读取响应
- stderr 用于日志输出，不作为协议通道

### 3. mcp/client.rs — McpClient

```rust
pub struct McpClient {
    transport: Box<dyn McpTransport>,
    server_info: Option<ServerInfo>,
    capabilities: Option<ServerCapabilities>,
}

impl McpClient {
    /// 通过 stdio 连接（启动子进程）
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

    /// 通过 WebSocket 连接
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

    /// MCP 握手：initialize + initialized 通知
    async fn initialize(&mut self) -> Result<()> {
        let result: InitializeResult = self.call("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "zero-nova",
                "version": env!("CARGO_PKG_VERSION")
            }
        })).await?;
        self.server_info = Some(result.server_info);
        self.capabilities = Some(result.capabilities);
        // 发送 initialized 通知（无 id，无需响应）
        self.notify("notifications/initialized", None).await?;
        Ok(())
    }

    /// 发现 server 暴露的工具列表
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let result: ListToolsResult = self.call("tools/list", json!({})).await?;
        Ok(result.tools)
    }

    /// 调用 server 上的工具
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<CallToolResult> {
        self.call("tools/call", json!({
            "name": name,
            "arguments": arguments
        })).await
    }

    /// 获取 server 信息
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// 关闭连接
    pub async fn close(self) -> Result<()> {
        self.transport.close().await
    }

    // 内部辅助
    async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T>;
    async fn notify(&self, method: &str, params: Option<Value>) -> Result<()>;
}
```

### 4. tool/mcp.rs — McpToolBridge

将 McpClient 暴露的工具桥接为标准 `Tool` trait 实现：

```rust
/// 单个 MCP 工具的 Tool trait 包装
struct McpTool {
    /// 共享的 MCP client（Arc 包装，多工具共用一个连接）
    client: Arc<McpClient>,
    /// 工具定义
    def: ToolDefinition,
}

#[async_trait]
impl Tool for McpTool {
    fn definition(&self) -> ToolDefinition {
        self.def.clone()
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput> {
        let result = self.client.call_tool(&self.def.name, input).await?;
        let text = result.content.iter()
            .filter_map(|c| match c {
                McpContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolOutput {
            content: text,
            is_error: result.is_error,
        })
    }
}

/// 桥接器：从 McpClient 生成 Tool 列表
pub struct McpToolBridge;

impl McpToolBridge {
    /// 发现 MCP server 的工具并转换为 Tool trait 实现
    pub async fn from_client(client: Arc<McpClient>) -> Result<Vec<Box<dyn Tool>>> {
        let mcp_tools = client.list_tools().await?;
        let tools: Vec<Box<dyn Tool>> = mcp_tools
            .into_iter()
            .map(|mcp_def| {
                let def = ToolDefinition {
                    name: mcp_def.name,
                    description: mcp_def.description,
                    input_schema: mcp_def.input_schema,
                };
                Box::new(McpTool {
                    client: Arc::clone(&client),
                    def,
                }) as Box<dyn Tool>
            })
            .collect();
        Ok(tools)
    }
}
```

### 5. mcp/mod.rs

```rust
pub mod client;
pub mod transport;
pub mod types;

pub use client::McpClient;
pub use types::{ServerInfo, McpToolDef};
```

## 集成流程

调用方使用 MCP 的完整流程：

```rust
use zero_nova::mcp::McpClient;
use zero_nova::tool::mcp::McpToolBridge;
use std::sync::Arc;

// 1. 连接 MCP server
let client = Arc::new(
    McpClient::connect_stdio("npx", &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]).await?
);

// 2. 桥接工具
let mcp_tools = McpToolBridge::from_client(client).await?;

// 3. 注入到 agent
for tool in mcp_tools {
    agent.register_tool(tool);
}

// 4. agent 正常使用，MCP 工具与内置工具无差异
let new_msgs = agent.run_turn(&history, "列出 /tmp 下的文件", tx).await?;
```

## 验证方式

1. 单元测试：JSON-RPC 序列化/反序列化、McpToolDef → ToolDefinition 转换
2. 集成测试：用 `@modelcontextprotocol/server-filesystem` 做端到端验证
   - connect_stdio → initialize 握手成功
   - list_tools 返回正确的工具列表
   - call_tool 执行 `list_directory` 并获取结果
   - McpToolBridge 生成的 Tool 可以通过 ToolRegistry 正常调度
3. 错误场景：server 未启动、server 崩溃、工具调用返回 is_error=true

## 交付物

`McpClient` + `McpToolBridge` 可用，MCP 工具与内置工具通过 ToolRegistry 统一调度。
