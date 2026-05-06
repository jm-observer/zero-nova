# Plan 1: 依赖引入与类型映射层

## Plan 编号与标题

Plan 1: 依赖引入与类型映射层

## 前置依赖

无

## 本次目标

1. 在 workspace 根 `Cargo.toml` 和 `nova-agent/Cargo.toml` 中添加 `async-openai` 依赖
2. 编写内部类型与 `async-openai` 类型之间的双向转换函数
3. 确保 `cargo check` 通过（新代码尚未被调用，仅编译验证）

## 涉及文件

| 文件 | 操作 |
|------|------|
| `Cargo.toml`（workspace 根） | 新增 `async-openai` workspace 依赖 |
| `crates/nova-agent/Cargo.toml` | 引用 workspace 依赖 |
| `crates/nova-agent/src/provider/openai_compat/conv.rs` | **新建** — 类型转换函数模块 |
| `crates/nova-agent/src/provider/openai_compat/mod.rs` | **新建** — 将 openai_compat 从单文件重构为目录模块 |

## 详细设计

### 1. 依赖声明

```toml
# workspace Cargo.toml [workspace.dependencies]
async-openai = { version = "0.36.1", default-features = false, features = ["chat-completion"] }
```

选择 `default-features = false` + `features = ["chat-completion"]`：
- 只启用 chat completion API 和类型，避免引入不需要的模块（audio、image、assistant 等）
- `chat-completion` feature 会自动启用 `chat-completion-types` 和内部 `_api` feature（包含 reqwest、tokio 等运行时依赖）

### 2. 类型映射关系

下表列出内部类型与 `async-openai` 类型的对应关系：

#### 请求侧

| 内部类型 | async-openai 类型 | 说明 |
|---------|-------------------|------|
| `Message` (Role::System) | `ChatCompletionRequestMessage::System` | system prompt |
| `Message` (Role::User) | `ChatCompletionRequestMessage::User` | 用户消息 |
| `Message` (Role::Assistant) + `ContentBlock::Text` | `ChatCompletionRequestMessage::Assistant` | 助手文本回复 |
| `Message` (Role::Assistant) + `ContentBlock::ToolUse` | `ChatCompletionRequestMessage::Assistant` with `tool_calls` | 助手工具调用 |
| `Message` (Role::User) + `ContentBlock::ToolResult` | `ChatCompletionRequestMessage::Tool` | 工具结果（每个 tool result 独立一条 Tool 消息） |
| `ContentBlock::Thinking` | **跳过** | 非标准字段，不映射 |
| `ToolDefinition` | `ChatCompletionTools::Function(ChatCompletionTool { function: FunctionObject })` | 工具定义 |
| `ModelConfig` | `CreateChatCompletionRequest` 的各字段 | model、max_tokens、temperature、top_p |

#### 响应侧

| async-openai 类型 | 内部类型 | 说明 |
|-------------------|---------|------|
| `ChatCompletionStreamResponseDelta.content` | `ProviderStreamEvent::TextDelta` | 文本增量 |
| `ChatCompletionMessageToolCallChunk` (首次出现 id) | `ProviderStreamEvent::ToolUseStart { id, name }` | 工具调用开始 |
| `ChatCompletionMessageToolCallChunk.function.arguments` | `ProviderStreamEvent::ToolUseInputDelta` | 工具参数增量 |
| `ChatChoiceStream.finish_reason` | `StopReason` 映射 | stop→EndTurn, length→MaxTokens, tool_calls→ToolUse |
| `CompletionUsage` | `Usage` | token 统计 |
| `CompletionUsage.prompt_tokens_details.cached_tokens` | `Usage.cache_read_input_tokens` | 缓存命中 token |
| `CompletionUsage.completion_tokens_details.reasoning_tokens` | `Usage.reasoning_tokens`（可选新增） | 推理 token（可选） |

#### Token Usage 映射

```
CompletionUsage {
    prompt_tokens: u32,          →  Usage.input_tokens: u64
    completion_tokens: u32,      →  Usage.output_tokens: u64
    total_tokens: u32,           →  （不映射，可由前两者相加得到）
    prompt_tokens_details: Option<PromptTokensDetails> {
        cached_tokens: Option<u32>  →  Usage.cache_read_input_tokens: Option<u64>
        audio_tokens: Option<u32>   →  （不映射）
    }
    completion_tokens_details: Option<CompletionTokensDetails> {
        reasoning_tokens: Option<u32>   →  存入 Usage.raw_provider_usage（备查）
        accepted_prediction_tokens      →  （不映射）
        rejected_prediction_tokens      →  （不映射）
        audio_tokens                    →  （不映射）
    }
}
```

### 3. conv.rs 转换函数签名

```rust
// === 请求侧转换 ===

/// 将内部 Message 列表转换为 async-openai 的 ChatCompletionRequestMessage 列表
pub fn messages_to_openai(messages: &[Message]) -> Vec<ChatCompletionRequestMessage>;

/// 将内部 ToolDefinition 列表转换为 async-openai 的 ChatCompletionTools 列表
pub fn tools_to_openai(tools: &[ToolDefinition]) -> Vec<ChatCompletionTools>;

/// 根据 ModelConfig 构建 CreateChatCompletionRequest
pub fn build_request(
    messages: &[Message],
    tools: &[ToolDefinition],
    config: &ModelConfig,
) -> CreateChatCompletionRequest;

// === 响应侧转换 ===

/// 将 async-openai 的 FinishReason 映射为内部 StopReason
pub fn map_finish_reason(reason: &FinishReason) -> StopReason;

/// 将 async-openai 的 CompletionUsage 映射为内部 Usage
pub fn map_usage(usage: &CompletionUsage) -> Usage;
```

### 4. 消息转换的关键逻辑

参考当前 `openai_compat.rs` 第 41-120 行的逻辑，转换需要处理以下场景：

1. **System 消息**：直接映射为 `ChatCompletionRequestMessage::System`
2. **纯文本 User/Assistant 消息**：映射为对应角色，content 为文本
3. **Assistant + ToolUse**：构建 `ChatCompletionRequestAssistantMessage`，设置 `tool_calls` 字段（`Vec<ChatCompletionMessageToolCalls>`），content 为可选文本
4. **ToolResult 消息**：每个 `ContentBlock::ToolResult` 生成一条独立的 `ChatCompletionRequestMessage::Tool`，包含 `tool_call_id` 和 `content`
5. **Thinking 块**：跳过（已确认去掉非标准字段支持）
6. **混合消息**（同一条消息包含 ToolResult + Text）：ToolResult 先输出为 Tool 消息，Text 部分单独输出为 User 消息（保持当前行为）

### 5. build_request 的关键逻辑

```rust
pub fn build_request(
    messages: &[Message],
    tools: &[ToolDefinition],
    config: &ModelConfig,
) -> CreateChatCompletionRequest {
    let openai_messages = messages_to_openai(messages);
    let openai_tools = if tools.is_empty() { None } else { Some(tools_to_openai(tools)) };

    CreateChatCompletionRequest {
        model: config.model.clone(),
        messages: openai_messages,
        tools: openai_tools,
        max_tokens: Some(config.max_tokens),
        temperature: config.temperature.map(|t| t as f32),
        top_p: config.top_p.map(|p| p as f32),
        stream: Some(true),
        stream_options: Some(ChatCompletionStreamOptions {
            include_usage: Some(true),
            ..Default::default()
        }),
        // thinking_budget 和 reasoning_effort 暂时不映射（已确认去掉非标准字段）
        ..Default::default()
    }
}
```

## 测试案例

### 1. 消息转换 — 纯文本对话

输入：
```
[
  Message { role: System, content: [Text("You are helpful")] },
  Message { role: User, content: [Text("Hello")] },
  Message { role: Assistant, content: [Text("Hi there")] },
]
```
预期：3 条 `ChatCompletionRequestMessage`，分别是 System、User、Assistant

### 2. 消息转换 — 工具调用 + 结果

输入：
```
[
  Message { role: Assistant, content: [Text("Let me search"), ToolUse { id: "t1", name: "search", input: {"q":"rust"} }] },
  Message { role: User, content: [ToolResult { tool_use_id: "t1", output: "found 10 results", is_error: false }] },
]
```
预期：
- 第一条：Assistant 消息带 content="Let me search" 和 tool_calls=[{id:"t1", function:{name:"search", arguments:"{\"q\":\"rust\"}"}}]
- 第二条：Tool 消息带 tool_call_id="t1" 和 content="found 10 results"

### 3. 消息转换 — Thinking 块被跳过

输入：
```
[
  Message { role: Assistant, content: [Thinking("let me think..."), Text("The answer is 42")] },
]
```
预期：1 条 Assistant 消息，content="The answer is 42"，无 thinking 内容

### 4. Usage 映射

输入：
```
CompletionUsage {
    prompt_tokens: 150,
    completion_tokens: 50,
    total_tokens: 200,
    prompt_tokens_details: Some(PromptTokensDetails { cached_tokens: Some(100), audio_tokens: None }),
    completion_tokens_details: None,
}
```
预期：
```
Usage {
    input_tokens: 150,
    output_tokens: 50,
    cache_creation_input_tokens: None,
    cache_read_input_tokens: Some(100),
    raw_provider_usage: Some(<原始 CompletionUsage 序列化值>),
}
```

### 5. ToolDefinition 转换

输入：
```
ToolDefinition { name: "read_file", description: "Read a file", input_schema: {"type":"object","properties":{"path":{"type":"string"}}} }
```
预期：`ChatCompletionTools::Function(ChatCompletionTool { function: FunctionObject { name: "read_file", description: Some("Read a file"), parameters: Some({...}), strict: None } })`

### 6. FinishReason 映射

| FinishReason | 预期 StopReason |
|-------------|----------------|
| `Stop` | `EndTurn` |
| `Length` | `MaxTokens` |
| `ToolCalls` | `ToolUse` |
| `ContentFilter` | `Unknown` |
