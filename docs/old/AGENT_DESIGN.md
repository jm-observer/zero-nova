# Zero-Nova Agent 设计需求文档

## 1. 项目定位

Zero-Nova 是一个面向个人助手场景的 Agent 运行时库（library crate），提供：
- 完整的 agentic loop（LLM 调用 → 工具执行 → 结果回传 → 循环）
- 动态工具/Skill 注册机制
- Channel 驱动的交互模型
- 可选的 MCP 协议支持

调用方通过 channel 与 agent 交互，自行管理会话历史、UI 渲染、持久化等上层逻辑。

## 2. 与 claw-code 的关系

### 2.1 混合策略

claw-code 的 crate（api, runtime, tools 等）之间深度耦合，无法作为独立 library 直接依赖。采用混合策略：

| 层面 | 策略 | 说明 |
|------|------|------|
| **Wire 类型** | 对齐 | `MessageRequest`、`StreamEvent`、`ToolDefinition` 等 wire format 与 claw-code 的 `api::types` 保持结构兼容，便于后续互操作 |
| **SSE Parser** | 提取适配 | claw-code 的 `sse.rs`（~130 行）逻辑完全自包含，仅依赖 `StreamEvent` 和 `ApiError` 类型，可直接提取并替换少量类型引用 |
| **Provider 实现** | 提取适配 | Anthropic/OpenAI 兼容的 HTTP 客户端实现，提取后替换 `runtime::TokenUsage` 等引用为本地定义 |
| **Agentic Loop** | 参考设计独立实现 | claw-code 的 `ConversationRuntime` 面向 CLI 同步 turn-by-turn 模式，zero-nova 需要 channel 驱动的异步模式，参考其 trait 泛型设计但独立实现 |
| **MCP Client** | 独立实现 | claw-code 的 MCP 深嵌 runtime（`McpServerManager`、hooks、permission 等），zero-nova 实现轻量版 JSON-RPC stdio/websocket client |
| **工具实现** | 独立实现 | 工具集面向个人助手场景，与 claw-code 的开发者工具集不同 |

### 2.2 参考但不依赖的核心模式

从 claw-code 借鉴的设计模式：

1. **Trait 泛型 Runtime**：`ConversationRuntime<C: ApiClient, T: ToolExecutor>` — 将 LLM 调用和工具执行解耦为 trait，便于测试和替换
2. **StreamEvent 枚举驱动**：用 tagged enum 统一表达流式事件，pattern matching 分发
3. **ToolDefinition 结构**：name + description + input_schema (JSON Schema) 的工具描述标准
4. **SSE 增量解析**：chunk-level buffer + frame 分割 + JSON 反序列化的三阶管线

## 3. 架构设计

### 3.1 模块结构

```
src/
├── lib.rs                  # 公开 API 导出
├── agent.rs                # AgentRuntime — agentic loop 核心
├── prompt.rs               # 系统提示词构建器
├── message.rs              # 消息与内容块类型定义
├── event.rs                # AgentEvent — 面向调用方的流式事件
├── provider/
│   ├── mod.rs              # LlmClient trait + ProviderClient 枚举
│   ├── types.rs            # Wire format 类型（与 claw api::types 对齐）
│   ├── sse.rs              # SSE 增量解析器（从 claw 提取适配）
│   ├── anthropic.rs        # Anthropic Messages API 实现
│   └── openai_compat.rs    # OpenAI 兼容实现（OpenAI/XAI/DashScope 等）
├── tool/
│   ├── mod.rs              # Tool trait + ToolRegistry
│   ├── builtin/            # 内置工具（feature-gated）
│   │   ├── mod.rs
│   │   ├── web_search.rs   # Web 搜索
│   │   ├── web_fetch.rs    # 网页抓取
│   │   ├── bash.rs         # 系统命令执行
│   │   └── file_ops.rs     # 文件读写
│   └── mcp.rs              # MCP 工具桥接（将 MCP server 暴露的工具统一为 Tool trait）
└── mcp/
    ├── mod.rs              # MCP 子系统入口
    ├── client.rs           # MCP JSON-RPC client（stdio + WebSocket）
    ├── transport.rs        # 传输层抽象
    └── types.rs            # MCP 协议类型（tool, resource, prompt）
```

### 3.2 核心类型

#### 3.2.1 消息类型（`message.rs`）

```rust
/// 对话消息角色
pub enum Role {
    User,
    Assistant,
}

/// 消息内容块
pub enum ContentBlock {
    /// 纯文本
    Text(String),
    /// 模型请求调用工具
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// 工具执行结果
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
}

/// 一条对话消息
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}
```

#### 3.2.2 流式事件（`event.rs`）

```rust
/// Agent 运行时向调用方推送的事件流
pub enum AgentEvent {
    /// LLM 输出的增量文本
    TextDelta(String),
    /// 开始执行工具
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// 工具执行完成
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    /// 本轮对话结束
    TurnComplete {
        /// 本轮新产生的消息（assistant 回复 + tool results）
        new_messages: Vec<Message>,
        /// Token 用量统计
        usage: Usage,
    },
    /// 运行时错误
    Error(AgentError),
}
```

### 3.3 LLM Provider 层（`provider/`）

#### 3.3.1 LlmClient trait

```rust
/// LLM 调用抽象。实现方负责 HTTP 通信和流式解析。
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 流式调用 LLM。
    /// - messages: 完整的对话历史（由调用方管理）
    /// - system: 系统提示词
    /// - tools: 当前可用工具列表
    /// - model_config: 模型参数（temperature 等，可选）
    ///
    /// 返回一个异步事件流。
    async fn stream(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>>;
}

/// 流式接收器。逐事件读取 LLM 响应。
#[async_trait]
pub trait StreamReceiver: Send {
    /// 获取下一个流式事件。返回 None 表示流结束。
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}
```

#### 3.3.2 Wire 类型对齐

`provider::types` 中的 wire format 与 claw-code `api::types` 结构对齐：

- `MessageRequest` — 发往 LLM API 的请求体
- `StreamEvent` — SSE 流事件（MessageStart, ContentBlockDelta, ToolUse 等）
- `ToolDefinition` — 工具描述（name, description, input_schema）
- `Usage` — token 用量统计

对齐意味着字段名、JSON tag、serde 属性保持一致，但不产生代码依赖。目的是：
1. 调试时可以直接对比两个系统的 JSON 报文
2. 未来如果 claw-code 将 types 拆为独立 crate，可以零成本切换

### 3.4 工具系统（`tool/`）

#### 3.4.1 Tool trait

```rust
/// 单个工具的抽象。实现此 trait 即可注册为 agent 可用工具。
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具的定义信息，会被序列化后发送给 LLM。
    fn definition(&self) -> ToolDefinition;

    /// 执行工具。input 为 LLM 生成的 JSON 参数。
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput>;
}

/// 工具执行结果
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}
```

#### 3.4.2 ToolRegistry

```rust
/// 工具注册表。支持运行时动态增删工具。
pub struct ToolRegistry { ... }

impl ToolRegistry {
    pub fn new() -> Self;

    /// 注册一个工具。如果 name 冲突，后注册的覆盖先注册的。
    pub fn register(&mut self, tool: Box<dyn Tool>);

    /// 批量注册
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>);

    /// 移除指定工具
    pub fn unregister(&mut self, name: &str) -> bool;

    /// 获取所有工具定义（传给 LLM）
    pub fn definitions(&self) -> Vec<ToolDefinition>;

    /// 按名称查找并执行工具
    pub async fn execute(&self, name: &str, input: serde_json::Value) -> Result<ToolOutput>;

    /// 合并另一个 registry 的全部工具（用于 MCP 工具注入）
    pub fn merge(&mut self, other: ToolRegistry);
}
```

#### 3.4.3 内置工具（feature-gated）

通过 Cargo features 按需启用，避免不需要的依赖：

```toml
[features]
default = []
tool-web-search = []      # Web 搜索
tool-web-fetch = ["scraper"]  # 网页抓取（需要 HTML parser）
tool-bash = []             # 系统命令执行
tool-file-ops = []         # 文件读写
builtin-tools = ["tool-web-search", "tool-web-fetch", "tool-bash", "tool-file-ops"]
mcp = []                   # MCP 协议支持
```

### 3.5 MCP 子系统（`mcp/`，feature-gated）

#### 3.5.1 定位

MCP 作为**补充工具来源**。当用户配置了 MCP server 时，agent 启动时连接这些 server，发现它们暴露的工具，并将这些工具桥接为标准的 `Tool` trait 实现注入 `ToolRegistry`。

#### 3.5.2 设计

```rust
/// MCP 客户端，管理与一个 MCP server 的连接
pub struct McpClient { ... }

impl McpClient {
    /// 通过 stdio 连接（启动子进程）
    pub async fn connect_stdio(command: &str, args: &[&str]) -> Result<Self>;

    /// 通过 WebSocket 连接
    pub async fn connect_ws(url: &str) -> Result<Self>;

    /// 发现 server 暴露的工具列表
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;

    /// 调用 server 上的工具
    pub async fn call_tool(&self, name: &str, input: Value) -> Result<String>;
}

/// 将 McpClient 暴露的工具桥接为 Tool trait
pub struct McpToolBridge { ... }

impl McpToolBridge {
    /// 连接 MCP server 并生成对应的 Tool 实现列表
    pub async fn from_client(client: McpClient) -> Result<Vec<Box<dyn Tool>>>;
}
```

MCP 工具对 agent 来说与内置工具无区别——都通过 `ToolRegistry.execute()` 统一调度。

### 3.6 AgentRuntime（`agent.rs`）

核心 agentic loop 实现。

#### 3.6.1 设计原则

1. **无状态**：runtime 不持有会话历史。调用方传入 `&[Message]`，runtime 返回本轮新消息
2. **Channel 驱动**：通过 `mpsc::Sender<AgentEvent>` 推送流式事件，调用方按需消费
3. **可组合**：LlmClient 和 ToolRegistry 均为外部注入，runtime 只负责编排

#### 3.6.2 接口

```rust
pub struct AgentRuntime<C: LlmClient> {
    client: C,
    tools: ToolRegistry,
    system_prompt: String,
    config: AgentConfig,
}

pub struct AgentConfig {
    /// 单轮对话中 LLM 调用的最大迭代次数（防止无限 tool-use 循环）
    pub max_iterations: usize,  // 默认 10
    /// 模型参数
    pub model: String,
    pub model_config: ModelConfig,
}

impl<C: LlmClient> AgentRuntime<C> {
    pub fn new(client: C, tools: ToolRegistry, system_prompt: String, config: AgentConfig) -> Self;

    /// 执行一轮对话。
    ///
    /// - history: 调用方维护的历史消息
    /// - user_input: 本轮用户输入
    /// - event_tx: 流式事件发送端
    ///
    /// 返回值: 本轮新产生的所有消息（Vec<Message>）
    /// 调用方拿到后自行决定是否追加到 history、是否持久化。
    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Vec<Message>>;

    /// 运行时替换工具集（例如 MCP server 重连后更新工具列表）
    pub fn set_tools(&mut self, tools: ToolRegistry);

    /// 运行时追加工具
    pub fn register_tool(&mut self, tool: Box<dyn Tool>);
}
```

#### 3.6.3 run_turn 内部流程

```
                    ┌─────────────────────────┐
                    │  调用方传入 history +     │
                    │  user_input              │
                    └───────────┬─────────────┘
                                │
                    ┌───────────▼─────────────┐
                    │  构造完整 messages       │
                    │  (history + user_msg)    │
                    └───────────┬─────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │         LLM stream 调用             │
              │  client.stream(messages, system,    │
              │                tools, config)       │
              └─────────────────┬──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │     消费流，推送 AgentEvent          │
              │  TextDelta → event_tx               │
              │  收集 assistant ContentBlocks       │
              └─────────────────┬──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │     提取 ToolUse blocks             │
              │  有 tool_use?                       │
              │  ├─ 否 → 跳出循环                   │
              │  └─ 是 → 继续                       │
              └─────────────────┬──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │  对每个 ToolUse:                     │
              │  1. 推送 ToolStart 事件              │
              │  2. registry.execute(name, input)   │
              │  3. 推送 ToolEnd 事件                │
              │  4. 构造 ToolResult ContentBlock     │
              └─────────────────┬──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │  将 assistant_msg + tool_results    │
              │  追加到 messages                    │
              │  iteration_count < max?             │
              │  ├─ 是 → 回到 LLM stream 调用       │
              │  └─ 否 → 跳出循环                   │
              └─────────────────┬──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │  推送 TurnComplete 事件              │
              │  返回 new_messages                  │
              └────────────────────────────────────┘
```

### 3.7 Skill 扩展机制（预留）

Skill 是高阶能力——组合多个工具调用完成一个复杂任务。当前版本预留 trait 定义，不实现调度逻辑。

```rust
/// Skill: 高阶能力抽象，可组合多个工具。
/// 与 Tool 的区别：Tool 是单次原子操作，Skill 是多步编排。
pub trait Skill: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;

    /// Skill 可以自行调用工具。
    /// 传入 ToolRegistry 的引用，由 Skill 内部编排工具调用顺序。
    async fn execute(
        &self,
        input: serde_json::Value,
        tools: &ToolRegistry,
    ) -> Result<String>;
}
```

Skill 在 agent 视角可以被包装为一个 Tool（对 LLM 来说就是一个工具），内部实现为多步操作。

### 3.8 系统提示词设计（`prompt.rs`）

#### 3.8.1 设计原则

系统提示词采用 **分段构建** 模式（参考 claw-code 的 `SystemPromptBuilder`），分为静态段和动态段：

- **静态段**：角色定义、行为准则、工具使用规范——构建时确定，运行期间不变
- **动态段**：环境信息、可用工具列表、用户自定义指令——每次 `run_turn` 前可更新

调用方可以：
1. 使用内置 builder 快速组装
2. 完全跳过 builder，直接传 `String` 给 `AgentRuntime`（system_prompt 本质就是一个字符串）

#### 3.8.2 SystemPromptBuilder

```rust
pub struct SystemPromptBuilder {
    /// 角色定义（静态段）
    role: Option<String>,
    /// 行为准则（静态段）
    guidelines: Vec<String>,
    /// 工具使用说明（动态段，由 ToolRegistry 自动生成）
    tool_instructions: Option<String>,
    /// 环境上下文（动态段）
    environment: Vec<(String, String)>,
    /// 用户自定义指令文件内容（动态段）
    custom_instructions: Vec<String>,
    /// 调用方追加的自由段落
    extra_sections: Vec<String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self;

    /// 使用面向个人助手的默认模板
    pub fn personal_assistant() -> Self;

    pub fn role(mut self, role: impl Into<String>) -> Self;
    pub fn guideline(mut self, text: impl Into<String>) -> Self;
    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self;
    pub fn extra_section(mut self, text: impl Into<String>) -> Self;

    /// 根据 ToolRegistry 自动生成工具说明段
    pub fn with_tools(mut self, registry: &ToolRegistry) -> Self;

    /// 渲染为最终的 system prompt 字符串
    pub fn build(&self) -> String;
}
```

#### 3.8.3 默认模板（`personal_assistant()`）

以下是 `SystemPromptBuilder::personal_assistant()` 的默认内容。各段独立，调用方可覆盖任何一段。

```text
# Role

You are a personal assistant agent. You help the user accomplish tasks by
breaking them into steps, using available tools, and reporting results clearly.

You operate in an agentic loop: you may call tools, observe their output, and
decide the next action until the task is complete. Only return a final answer
when you are confident the task is done or you need clarification from the user.

# Guidelines

- Think step-by-step before acting. When a task is complex, outline your plan
  first, then execute.
- Use the most appropriate tool for each step. If no tool fits, say so instead
  of guessing.
- When a tool call fails, diagnose the cause before retrying or switching
  approach.
- Be concise in your responses. Avoid repeating tool output verbatim — instead,
  summarize the key findings.
- If you are unsure about the user's intent, ask for clarification rather than
  making assumptions.
- Respect the user's system: do not modify, delete, or overwrite files unless
  the user explicitly requests it.
- When presenting information from web searches or fetches, note the source so
  the user can verify.
- Do not fabricate information. If you cannot find an answer, say so.

# Tool usage

You have access to the following tools. To use a tool, output a tool_use block
with the tool name and a JSON object matching its input_schema.

{tool_descriptions}

Each tool call will be executed and the result returned to you. You may then
call more tools or provide a final response.

# Environment

{environment_entries}

# Custom instructions

{custom_instructions}
```

**各段说明**：

| 段落 | 内容 | 来源 |
|------|------|------|
| **Role** | 角色定位：个人助手，agentic loop 行为模式 | 内置默认 / 调用方覆盖 |
| **Guidelines** | 行为准则：逐步思考、工具优先、失败诊断、简洁回复、不捏造 | 内置默认 / 调用方追加 |
| **Tool usage** | 可用工具列表及其 description + input_schema | 由 `with_tools()` 从 ToolRegistry 自动生成 |
| **Environment** | 运行环境：日期、平台、工作目录等键值对 | 调用方通过 `environment()` 注入 |
| **Custom instructions** | 用户自定义指令（类似 claw 的 CLAUDE.md） | 调用方通过 `custom_instruction()` 注入 |

#### 3.8.4 工具说明自动生成

`with_tools()` 遍历 `ToolRegistry::definitions()`，为每个工具生成：

```text
## tool_name

description_text

Input schema:
```json
{ ... input_schema ... }
```​
```

这段会替换模板中的 `{tool_descriptions}` 占位符。当工具集变化时（如 MCP server 重连），调用方重新调用 `with_tools()` 即可。

#### 3.8.5 调用方用例

**最小用法**：
```rust
let prompt = SystemPromptBuilder::personal_assistant()
    .with_tools(&tools)
    .environment("date", "2026-04-14")
    .environment("platform", "Windows 11")
    .build();
```

**完全自定义**：
```rust
let prompt = SystemPromptBuilder::new()
    .role("You are a research analyst specialized in technology trends.")
    .guideline("Always cite sources with URLs.")
    .guideline("Present findings in structured tables when comparing items.")
    .with_tools(&tools)
    .environment("date", "2026-04-14")
    .custom_instruction("Focus on AI and semiconductor industries.")
    .build();
```

**跳过 builder，直接传字符串**：
```rust
let agent = AgentRuntime::new(client, tools, "You are a helpful assistant.".into(), config);
```

## 4. 测试二进制（`nova-cli`）

### 4.1 定位

`nova-cli` 是一个独立的 bin target，用于手动验证 zero-nova library 的各项能力。它不是产品，是开发期的测试和演示工具。

### 4.2 Cargo 配置

```toml
# Cargo.toml 中新增
[[bin]]
name = "nova-cli"
path = "src/bin/nova_cli.rs"
required-features = ["cli"]

[features]
cli = ["builtin-tools", "mcp", "dep:clap", "dep:rustyline"]

[dependencies]
clap = { version = "4", features = ["derive"], optional = true }
rustyline = { version = "15", optional = true }
```

### 4.3 文件位置

```
src/
└── bin/
    └── nova_cli.rs     # 测试二进制入口
```

不新增其他文件。所有逻辑集中在 `nova_cli.rs` 单文件内，保持简单。

### 4.4 命令设计

```
nova-cli [OPTIONS] <COMMAND>

Options:
  --model <MODEL>       模型名称 [default: claude-sonnet-4-20250514]
  --base-url <URL>      自定义 API base URL
  --verbose             打印完整的 tool input/output

Commands:
  chat                  交互式对话（REPL 模式）
  run <PROMPT>          单轮执行（one-shot 模式）
  tools                 列出当前已注册的全部工具
  mcp-test <CMD>        测试 MCP server 连接与工具发现
```

#### 4.4.1 `chat` — REPL 交互

```
$ nova-cli chat
[nova] tools loaded: web_search, web_fetch, bash, read_file, write_file
[nova] type /help for commands, /quit to exit

you> 帮我搜索 Rust 2026 年的新特性
[tool: web_search] query="Rust programming language 2026 new features"
[tool: web_fetch] url="https://blog.rust-lang.org/..."

Rust 2026 edition 的主要新特性包括：
1. ...
2. ...

[tokens: input=1234, output=567]

you> 把上面的内容保存到 /tmp/rust2026.md
[tool: write_file] path="/tmp/rust2026.md"
已写入 /tmp/rust2026.md（1.2 KB）

you> /quit
```

内置 REPL 命令：

| 命令 | 作用 |
|------|------|
| `/quit` | 退出 |
| `/tools` | 列出当前工具 |
| `/clear` | 清空对话历史 |
| `/history` | 打印当前历史消息数量和 token 估算 |
| `/mcp add <cmd> [args...]` | 运行时连接一个 MCP server |
| `/mcp list` | 列出已连接的 MCP server 及其工具 |
| `/mcp remove <name>` | 断开一个 MCP server |
| `/prompt` | 打印当前完整的 system prompt |

#### 4.4.2 `run` — One-shot 执行

```
$ nova-cli run "搜索今天的科技新闻，整理成 5 条摘要"
[tool: web_search] ...
[tool: web_fetch] ...

1. Apple 发布...
2. ...

$ echo $?
0
```

适用于脚本集成和自动化测试。

#### 4.4.3 `tools` — 工具清单

```
$ nova-cli tools
Built-in tools:
  web_search    Search the web using a search engine
  web_fetch     Fetch and extract content from a URL
  bash          Execute a shell command
  read_file     Read the contents of a file
  write_file    Write content to a file

MCP tools: (none connected)
```

#### 4.4.4 `mcp-test` — MCP 连接测试

```
$ nova-cli mcp-test npx -y @modelcontextprotocol/server-filesystem /tmp
Connecting to MCP server: npx -y @modelcontextprotocol/server-filesystem /tmp
Connected. Server capabilities:
  tools: 4
    - read_file: Read a file from the filesystem
    - write_file: Write content to a file
    - list_directory: List directory contents
    - search_files: Search for files matching a pattern

Testing tool call: list_directory {"path": "/tmp"}
  Result: OK (12 entries)

All checks passed.
```

### 4.5 内部实现概要

```rust
// src/bin/nova_cli.rs 伪代码结构
use zero_nova::*;

#[derive(clap::Parser)]
struct Cli {
    #[arg(long, default_value = "claude-sonnet-4-20250514")]
    model: String,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Chat,
    Run { prompt: String },
    Tools,
    McpTest { cmd: Vec<String> },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 构建 provider
    let client = make_client(&cli)?;

    // 注册内置工具
    let mut tools = ToolRegistry::new();
    register_builtin_tools(&mut tools);

    // 构建 system prompt
    let prompt = SystemPromptBuilder::personal_assistant()
        .with_tools(&tools)
        .environment("date", current_date())
        .environment("platform", std::env::consts::OS)
        .build();

    let config = AgentConfig { model: cli.model, ..Default::default() };
    let mut agent = AgentRuntime::new(client, tools, prompt, config);

    match cli.command {
        Command::Chat => run_repl(&mut agent, cli.verbose).await,
        Command::Run { prompt } => run_oneshot(&mut agent, &prompt, cli.verbose).await,
        Command::Tools => print_tools(&agent),
        Command::McpTest { cmd } => test_mcp_connection(&cmd).await,
    }
}
```

### 4.6 验证矩阵

nova-cli 需要覆盖以下测试场景：

| 场景 | 命令 | 验证点 |
|------|------|--------|
| 纯对话 | `run "你好"` | LLM 调用 + 流式输出正常 |
| Web 搜索 | `run "搜索 Rust async"` | web_search 工具被调用，返回结果 |
| 网页抓取 | `run "总结 https://example.com 的内容"` | web_fetch 工具被调用 |
| 命令执行 | `run "列出当前目录的文件"` | bash 工具执行 `ls` |
| 文件读取 | `run "读取 /tmp/test.txt"` | read_file 工具调用 |
| 文件写入 | `run "把 hello 写入 /tmp/out.txt"` | write_file 工具调用，文件实际写入 |
| 多轮工具链 | `chat` 中连续指令 | 多轮 history 正确传递，工具链式调用 |
| MCP 连接 | `mcp-test npx ... server-filesystem` | stdio 子进程启动、工具发现、工具调用 |
| MCP 动态注入 | `chat` 中 `/mcp add ...` | 运行时工具集扩展，LLM 能使用新工具 |
| 多工具并行 | `run "搜索 X 然后保存到文件"` | 多个工具在一轮中被依次调用 |
| 错误恢复 | `run "读取 /nonexistent"` | 工具返回 is_error=true，LLM 能处理错误 |

## 5. 调用方集成示例

### 5.1 最小用法

```rust
use zero_nova::{AgentRuntime, AgentConfig, AgentEvent, Message, ToolRegistry};
use zero_nova::provider::AnthropicClient;
use tokio::sync::mpsc;

async fn run() -> anyhow::Result<()> {
    let client = AnthropicClient::from_env()?;
    let tools = ToolRegistry::new();
    let prompt = SystemPromptBuilder::personal_assistant()
        .with_tools(&tools)
        .environment("date", "2026-04-14")
        .build();
    let config = AgentConfig::default();
    let agent = AgentRuntime::new(client, tools, prompt, config);

    let mut history: Vec<Message> = vec![];
    let (tx, mut rx) = mpsc::channel(256);

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::TextDelta(t) => print!("{t}"),
                AgentEvent::ToolStart { name, .. } => println!("\n[tool: {name}]"),
                AgentEvent::TurnComplete { usage, .. } => {
                    println!("\n[tokens: {}]", usage.total_tokens());
                }
                _ => {}
            }
        }
    });

    let new_msgs = agent.run_turn(&history, "帮我搜索 Rust async 最佳实践", tx).await?;
    history.extend(new_msgs);

    Ok(())
}
```

### 5.2 动态注册自定义工具

```rust
use zero_nova::tool::{Tool, ToolDefinition, ToolOutput};

struct MyCalendarTool { /* ... */ }

#[async_trait]
impl Tool for MyCalendarTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calendar_query".into(),
            description: Some("查询用户日程安排".into()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "date": { "type": "string", "description": "日期，格式 YYYY-MM-DD" }
                },
                "required": ["date"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput> {
        let date = input["date"].as_str().unwrap_or("today");
        Ok(ToolOutput {
            content: format!("{date} 的日程: 10:00 开会, 14:00 review"),
            is_error: false,
        })
    }
}

agent.register_tool(Box::new(MyCalendarTool {}));
```

### 5.3 接入 MCP Server

```rust
use zero_nova::mcp::{McpClient, McpToolBridge};

let mcp = McpClient::connect_stdio("npx", &["-y", "@modelcontextprotocol/server-filesystem"]).await?;
let mcp_tools = McpToolBridge::from_client(mcp).await?;

for tool in mcp_tools {
    agent.register_tool(tool);
}
```

## 6. 扩展性设计总结

| 扩展场景 | 方式 | 侵入性 |
|---------|------|--------|
| 新增工具 | 实现 `Tool` trait，调用 `register_tool()` | 零侵入 |
| 新增 Skill | 实现 `Skill` trait，包装为 Tool 注册 | 零侵入 |
| 新增 LLM 后端 | 实现 `LlmClient` trait | 零侵入 |
| 接入 MCP Server | `McpClient` + `McpToolBridge` | 零侵入 |
| 自定义 session 管理 | 调用方自行维护 `Vec<Message>` | 不涉及 runtime |
| 自定义 system prompt | 传字符串或用 `SystemPromptBuilder` | 零侵入 |
| 工具执行前后的 hook | 后续可在 ToolRegistry 添加中间件链 | 预留 |
| 热更新工具集 | `set_tools()` / `register_tool()` / `unregister()` | 零侵入 |

## 7. 依赖规划

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
async-trait = "0.1"
log = "0.4"

# CLI 专用（可选）
clap = { version = "4", features = ["derive"], optional = true }
rustyline = { version = "15", optional = true }

[dev-dependencies]
tokio-test = "0.4"

[features]
default = []
tool-web-search = []
tool-web-fetch = ["dep:scraper"]
tool-bash = []
tool-file-ops = []
builtin-tools = ["tool-web-search", "tool-web-fetch", "tool-bash", "tool-file-ops"]
mcp = []
cli = ["builtin-tools", "mcp", "dep:clap", "dep:rustyline"]
```

## 8. 实现计划

### Plan 1: 基础骨架（类型 + Provider + Agentic Loop）

**目标**：跑通最小闭环 — 用户输入 → LLM 响应 → 流式输出，无工具调用。

**范围**：
1. `message.rs` — Message、Role、ContentBlock 类型定义
2. `event.rs` — AgentEvent 枚举
3. `provider/types.rs` — wire format 类型（与 claw-code 对齐）
4. `provider/sse.rs` — SSE 增量解析器（从 claw-code 提取适配）
5. `provider/anthropic.rs` — Anthropic streaming client（LlmClient trait 实现）
6. `provider/mod.rs` — LlmClient trait、StreamReceiver trait、ProviderClient
7. `agent.rs` — AgentRuntime + run_turn 循环（含工具执行分支，但此阶段无工具可调用）
8. `prompt.rs` — SystemPromptBuilder + personal_assistant 默认模板
9. `lib.rs` — 公开 API 导出

**验证方式**：单元测试 + 写一个 `#[tokio::test]` 集成测试，用 mock LLM client 验证 run_turn 流程。

**交付物**：`zero_nova::AgentRuntime` 可以完成一轮纯对话。

### Plan 2: 工具系统 + 内置工具

**目标**：Agent 能调用工具，完成 搜索 → 抓取 → 文件写入 等完整链路。

**前置**：Plan 1 完成。

**范围**：
1. `tool/mod.rs` — Tool trait、ToolOutput、ToolRegistry
2. `tool/builtin/bash.rs` — 系统命令执行工具
3. `tool/builtin/file_ops.rs` — read_file、write_file 工具
4. `tool/builtin/web_search.rs` — Web 搜索工具（调用搜索 API 或 scraping）
5. `tool/builtin/web_fetch.rs` — 网页抓取工具（HTML → 文本提取）
6. `tool/builtin/mod.rs` — `register_builtin_tools()` 便捷函数
7. agent.rs 中的工具执行路径验证（ToolUse → execute → ToolResult 循环）
8. system prompt 中 `{tool_descriptions}` 自动生成逻辑

**验证方式**：集成测试验证工具注册、执行、动态增删。

**交付物**：Agent 能在对话中自动调用工具并利用结果继续推理。

### Plan 3: MCP 支持

**目标**：Agent 能连接外部 MCP server，发现并使用其工具。

**前置**：Plan 2 完成。

**范围**：
1. `mcp/types.rs` — MCP JSON-RPC 协议类型
2. `mcp/transport.rs` — Transport trait + StdioTransport + WebSocketTransport
3. `mcp/client.rs` — McpClient（initialize、list_tools、call_tool）
4. `tool/mcp.rs` — McpToolBridge（将 MCP 工具适配为 Tool trait）
5. `mcp/mod.rs` — 子系统入口

**验证方式**：用 `@modelcontextprotocol/server-filesystem` 做端到端测试。

**交付物**：`McpClient` + `McpToolBridge` 可用，MCP 工具与内置工具统一调度。

### Plan 4: 测试二进制 nova-cli

**目标**：提供可交互的 CLI 工具，手动验证所有功能。

**前置**：Plan 3 完成（或至少 Plan 2 完成，MCP 部分可后补）。

**范围**：
1. `src/bin/nova_cli.rs` — CLI 入口，chat / run / tools / mcp-test 四个子命令
2. REPL 循环 + `/mcp` `/tools` `/clear` 等斜杠命令
3. 流式输出渲染（AgentEvent → stdout）
4. 验证矩阵中各场景的端到端跑通

**验证方式**：手动执行验证矩阵中的全部场景。

**交付物**：`cargo run --bin nova-cli --features cli -- chat` 可用。

### Plan 5: OpenAI 兼容后端 + Skill 机制（后续）

**目标**：扩展 provider 覆盖面，引入 Skill 高阶编排。

**范围**：
1. `provider/openai_compat.rs` — OpenAI / XAI / DashScope 兼容实现
2. Skill trait 正式实现 + SkillAsToolWrapper
3. nova-cli 中 `--model grok-3` 等非 Anthropic 模型支持

## 9. 设计约束与非目标

### 约束
- 纯 library crate，不修改 `main.rs`
- nova-cli 是独立 bin target，通过 feature gate 控制
- Session 管理由调用方负责，runtime 无状态
- 所有内置工具通过 Cargo features 可选启用
- Wire 类型与 claw-code 结构对齐但无代码依赖
- system prompt 可完全自定义，内置模板只是便捷方案

### 非目标（当前版本不做）
- 权限系统（调用方自行在 Tool 实现中处理）
- 插件系统（通过 Tool trait 动态注册已满足需求）
- 会话持久化 / compaction
- Prompt caching（后续按需引入）
- Telemetry / 计费
