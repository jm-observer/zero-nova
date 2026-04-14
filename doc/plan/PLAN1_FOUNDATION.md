# Plan 1: 基础骨架（类型 + Provider + Agentic Loop）

## 目标

跑通最小闭环 — 用户输入 → LLM 响应 → 流式输出，无工具调用。

## 前置

无。

## 范围

| # | 文件 | 内容 |
|---|------|------|
| 1 | `src/message.rs` | Message、Role、ContentBlock 类型定义 |
| 2 | `src/event.rs` | AgentEvent 枚举 |
| 3 | `src/provider/types.rs` | Wire format 类型（与 claw-code `api::types` 对齐） |
| 4 | `src/provider/sse.rs` | SSE 增量解析器（从 claw-code 提取适配） |
| 5 | `src/provider/anthropic.rs` | Anthropic streaming client（LlmClient trait 实现） |
| 6 | `src/provider/mod.rs` | LlmClient trait、StreamReceiver trait、ProviderClient |
| 7 | `src/agent.rs` | AgentRuntime + run_turn 循环（含工具执行分支，但此阶段无工具可调用） |
| 8 | `src/prompt.rs` | SystemPromptBuilder + personal_assistant 默认模板 |
| 9 | `src/lib.rs` | 公开 API 导出 |

## 详细设计

### 1. message.rs

```rust
pub enum Role {
    User,
    Assistant,
}

pub enum ContentBlock {
    Text(String),
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, output: String, is_error: bool },
}

pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}
```

### 2. event.rs

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    TurnComplete { new_messages: Vec<Message>, usage: Usage },
    Error(AgentError),
}
```

### 3. provider/types.rs

与 claw-code `api::types` 字段名、JSON tag、serde 属性保持一致：

- `MessageRequest` — 发往 LLM API 的请求体
- `InputMessage` / `InputContentBlock` — 请求侧消息格式
- `StreamEvent` — SSE 流事件枚举（MessageStart, ContentBlockStart, ContentBlockDelta, ContentBlockStop, MessageDelta, MessageStop）
- `ToolDefinition` — 工具描述（name, description, input_schema）
- `Usage` — token 用量统计（input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens）
- `ToolChoice` — Auto / Any / Tool { name }

### 4. provider/sse.rs

从 claw-code `gits/claw-code/rust/crates/api/src/sse.rs` 提取适配：

- `SseParser` — chunk-level buffer + frame 分割 + JSON 反序列化
- `parse_frame()` — 单帧解析函数
- 替换 `ApiError` → 本地错误类型
- 替换 `StreamEvent` → 本地 `provider::types::StreamEvent`
- 逻辑不变（~130 行），仅替换类型引用

### 5. provider/anthropic.rs

```rust
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn from_env() -> Result<Self>;
    pub fn new(api_key: String, base_url: String) -> Self;
}

impl LlmClient for AnthropicClient {
    async fn stream(...) -> Result<Box<dyn StreamReceiver>>;
}

struct AnthropicStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
}

impl StreamReceiver for AnthropicStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}
```

关键实现要点：
- HTTP POST `{base_url}/v1/messages`，body 为 `MessageRequest` with `stream: true`
- Header: `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`
- 响应体通过 `reqwest::Response::chunk()` 增量读取，喂入 `SseParser`
- `ProviderStreamEvent` 从 `StreamEvent` 转换，提取出 text delta / tool_use 等高层事件

### 6. provider/mod.rs

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>>;
}

#[async_trait]
pub trait StreamReceiver: Send {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}

pub struct ModelConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
}
```

`ProviderStreamEvent` 是从 wire-level `StreamEvent` 提炼出的高层事件：

```rust
pub enum ProviderStreamEvent {
    TextDelta(String),
    ToolUseStart { id: String, name: String },
    ToolUseInputDelta(String),
    ToolUseEnd,
    MessageComplete { usage: Usage },
}
```

### 7. agent.rs

```rust
pub struct AgentRuntime<C: LlmClient> {
    client: C,
    tools: ToolRegistry,
    system_prompt: String,
    config: AgentConfig,
}

pub struct AgentConfig {
    pub max_iterations: usize,  // 默认 10
    pub model_config: ModelConfig,
}

impl<C: LlmClient> AgentRuntime<C> {
    pub fn new(client: C, tools: ToolRegistry, system_prompt: String, config: AgentConfig) -> Self;
    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Vec<Message>>;
    pub fn set_tools(&mut self, tools: ToolRegistry);
    pub fn register_tool(&mut self, tool: Box<dyn Tool>);
}
```

run_turn 流程见 [AGENT_DESIGN.md 第 3.6.3 节](./AGENT_DESIGN.md#363-run_turn-内部流程)。

### 8. prompt.rs

```rust
pub struct SystemPromptBuilder { ... }

impl SystemPromptBuilder {
    pub fn new() -> Self;
    pub fn personal_assistant() -> Self;
    pub fn role(mut self, role: impl Into<String>) -> Self;
    pub fn guideline(mut self, text: impl Into<String>) -> Self;
    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self;
    pub fn extra_section(mut self, text: impl Into<String>) -> Self;
    pub fn with_tools(mut self, registry: &ToolRegistry) -> Self;
    pub fn build(&self) -> String;
}
```

默认模板详见 [AGENT_DESIGN.md 第 3.8.3 节](./AGENT_DESIGN.md#383-默认模板personal_assistant)。

### 9. lib.rs

```rust
pub mod agent;
pub mod event;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod tool;

pub use agent::{AgentConfig, AgentRuntime};
pub use event::AgentEvent;
pub use message::{ContentBlock, Message, Role};
pub use prompt::SystemPromptBuilder;
pub use provider::{LlmClient, ModelConfig, StreamReceiver};
pub use tool::{Tool, ToolDefinition, ToolOutput, ToolRegistry};
```

## 验证方式

1. 单元测试：SSE parser、Message 序列化/反序列化、SystemPromptBuilder 渲染
2. 集成测试：`#[tokio::test]`，用 mock LlmClient 验证 run_turn 完整流程
   - mock client 返回固定的 TextDelta 事件序列
   - 验证 event_tx 收到正确的 AgentEvent 序列
   - 验证 run_turn 返回的 `Vec<Message>` 结构正确

## 交付物

`zero_nova::AgentRuntime` 可以完成一轮纯对话（无工具调用）。
