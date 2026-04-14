# Zero-Nova 使用与测试指南

本文档介绍如何配置和测试 Zero-Nova 代理的核心功能，包括自定义 MCP 服务连接、内置工具测试以及 Web 探索能力。

## 1. 基础配置

在运行 Nova CLI 或任何基于 `zero-nova` 的应用前，需确保以下基础环境变量已设置：

| 环境变量 | 说明 | 示例 |
| :--- | :--- | :--- |
| `API_KEY` | LLM 提供商的 API Key (Anthropic) | `sk-ant-api03-...` |
| `SEARCH_API_KEY` | Web 搜索工具所需的 API Key (Brave) | `brave_api_key_...` |

> [!NOTE]
> `nova-cli` 默认连接 Anthropic 的 API，并会自动检测上述环境变量。

## 2. MCP (Model Context Protocol) 测试与使用

MCP 允许 Agent 连接到外部工具服务器。Zero-Nova 支持基于 `Stdio` 和 `WebSocket` 的传输协议。

### 2.1 使用 CLI 测试 MCP 连接

您可以使用 `nova-cli` 的 `mcp-test` 子命令快速验证一个 MCP Server 是否能够正常通信：

```powershell
# 测试本地 Filesystem MCP (需安装 Node.js/npx)
cargo run --bin nova-cli mcp-test npx -y @modelcontextprotocol/server-filesystem C:\Users

# 测试远程 WebSocket MCP
# 注：CLI sub-command 目前主要侧重 stdio 测试，WS 测试建议通过代码集成验证
```

### 2.2 代码中集成 MCP

在开发中，您可以通过以下步骤将 MCP Server 加载到 Agent 中：

```rust
use zero_nova::mcp::client::McpClient;
use zero_nova::tool::mcp::McpToolBridge;
use std::sync::Arc;

// 1. 连接到 MCP Server (以 stdio 为例)
let client = Arc::new(
    McpClient::connect_stdio("npx", &["-y", "@modelcontextprotocol/server-filesystem", "C:\\"]).await?
);

// 2. 发现并转换工具
let mcp_tools = McpToolBridge::from_client(client).await?;

// 3. 注册到 ToolRegistry
let mut registry = ToolRegistry::new();
registry.register_many(mcp_tools);
```

## 3. Web 搜索工具测试

Web 搜索是内置工具（Builtin Tool），依赖环境变量进行初始化。

### 3.1 配置搜索后端

默认使用 **Brave Search API**。

*   **配置项**：
    *   `SEARCH_API_KEY` (必填)
    *   `SEARCH_ENDPOINT` (可选，默认为 Brave 官方 V1 地址)

### 3.2 运行测试

在设置好环境变量后，启动 `nova-cli` 并执行带有搜索需求的指令：

```powershell
# 设置环境变量 (CMD/PowerShell 示例)
$env:SEARCH_API_KEY="your_brave_key"

# 运行单次任务并开启详细模式查看工具调用过程
cargo run --bin nova-cli run "查找最近一周关于 Rust 编程语言的新闻" --verbose
```

## 4. Nova CLI 常用操作

`nova-cli` 是用于验证 `zero-nova` 库功能的交互式终端。

| 子命令/快捷指令 | 说明 |
| :--- | :--- |
| `chat` | 进入交互式 REPL 模式 |
| `run "<prompt>"` | 一次性执行指令并退出 |
| `tools` | 列出当前所有已注册的工具（内置 + MCP） |
| `mcp-test <cmd>` | 专门用于测试 MCP Server 通信 |
| `/quit` (REPL 内) | 退出交互模式 |
| `/clear` (REPL 内) | 清除对话历史 |
| `/prompt` (REPL 内) | 查看当前生成的系统提示词（含工具定义） |

## 5. 常见问题排查建议

*   **工具未显示**：检查 `SEARCH_API_KEY` 是否正确导出，Web 搜索工具在 key 缺失时不会注册。
*   **MCP 链接超时**：确保 MCP Server 命令能在本地终端正常独立运行，检查是否需要代理。
*   **LLM 响应异常**：检查 `API_KEY` 权限，或者通过 `--base-url` 切换至兼容的端点。

---
*Last updated: 2026-04-15*
