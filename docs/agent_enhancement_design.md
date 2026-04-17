# AgentRuntime 处理逻辑增强设计文档

> 版本: v1.0 | 日期: 2026-04-17

## 1. 背景与动机

当前 `AgentRuntime::run_turn()` (`src/agent.rs:55-228`) 实现了一个基础的 agentic 循环：流式接收 LLM 响应、收集 tool call、并行执行 tool、将结果追加到上下文后循环调用 LLM。该循环能完成基本任务，但在健壮性、可观测性和可控性方面存在明显短板。

### 1.1 现状问题

| # | 问题 | 影响 | 代码位置 |
|---|------|------|----------|
| P1 | Tool 结果顺序随机 | `FuturesUnordered` 按完成顺序返回，tool_result 与 tool_use 顺序不匹配，可能导致模型混淆 | `agent.rs:155-204` |
| P2 | 无累计 token 用量 | `last_usage` 每次迭代被覆盖，多轮迭代的 Turn 只报告最后一次调用的 token 数，用量统计不准确 | `agent.rs:91,111-113` |
| P3 | 不感知 stop_reason | `ProviderStreamEvent::MessageComplete` 只携带 `Usage`，无法区分 end_turn / max_tokens / tool_use | `provider/mod.rs:36` |
| P4 | max_tokens 截断无处理 | 当 LLM 输出被 max_tokens 截断时，agent 将不完整的文本视为正常结束，不会自动续写 | `agent.rs:140-151` |
| P5 | 迭代上限静默退出 | 达到 `max_iterations` 后循环结束，不发送任何通知事件，调用方无法感知 | `agent.rs:213-223` |
| P6 | Tool 执行无超时 | 慢速 HTTP 请求（web_search/web_fetch）或阻塞的 bash 命令可以无限期挂起整个 agent turn | `agent.rs:173` |
| P7 | 无取消机制 | `run_turn()` 启动后无法从外部中止；WebSocket 客户端断开后 agent 仍继续消耗资源 | `agent.rs:55-60` |

## 2. 改进方案总览

```
┌──────────────────────────────────────────────────────────┐
│                    AgentRuntime::run_turn()               │
│                                                          │
│  ┌─ 新增 ─────────────────────────────────────────────┐  │
│  │ CancellationToken 检查 ←─── 外部取消信号            │  │
│  │ cumulative_usage 累加 ←─── 跨迭代 token 统计        │  │
│  │ stop_reason 感知    ←─── MaxTokens 自动续写         │  │
│  │ tool 索引排序       ←─── 保持 tool_result 顺序      │  │
│  │ tool 超时包裹       ←─── tokio::time::timeout       │  │
│  │ 迭代上限通知        ←─── IterationLimitReached 事件  │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## 3. 详细设计

### 3.1 Tool 结果顺序保持

**问题根因：** `FuturesUnordered` 是一个无序并发执行器，futures 按完成时间先后返回结果，而非提交顺序。

**解决方案：** 在将 tool call 提交到 `FuturesUnordered` 时，通过 `enumerate()` 为每个 future 分配原始索引。future 返回 `(usize, ContentBlock)` 元组，收集完毕后按索引排序。

**伪代码：**

```rust
// 提交时携带索引
for (call_index, (id, name, input_json)) in tool_calls.into_iter().enumerate() {
    tool_results_fut.push(async move {
        // ... execute tool ...
        (call_index, ContentBlock::ToolResult { tool_use_id: id, output, is_error })
    });
}

// 收集后排序
let mut indexed_results: Vec<(usize, ContentBlock)> = Vec::new();
while let Some(pair) = tool_results_fut.next().await {
    indexed_results.push(pair);
}
indexed_results.sort_by_key(|(idx, _)| *idx);
let tool_result_blocks: Vec<ContentBlock> = indexed_results.into_iter().map(|(_, b)| b).collect();
```

**影响范围：** 仅 `src/agent.rs`，不改变任何公共接口。

---

### 3.2 累计 Token 用量统计

**问题根因：** `last_usage` 在每次迭代的 `MessageComplete` 事件中被直接覆盖（`agent.rs:112`），多迭代 turn 丢失了前序迭代的 token 消耗。

**解决方案：** 在 `run_turn` 函数作用域内声明 `cumulative_usage: Usage`，每次迭代累加：

```rust
let mut cumulative_usage = Usage::default(); // 循环外声明

// 在 MessageComplete 匹配分支中：
ProviderStreamEvent::MessageComplete { usage, .. } => {
    cumulative_usage.input_tokens += usage.input_tokens;
    cumulative_usage.output_tokens += usage.output_tokens;
    cumulative_usage.cache_creation_input_tokens += usage.cache_creation_input_tokens;
    cumulative_usage.cache_read_input_tokens += usage.cache_read_input_tokens;
}
```

`TurnComplete` 事件始终使用 `cumulative_usage` 而非 `last_usage`。

**影响范围：** 仅 `src/agent.rs`。`Usage` 已有 `Default` derive（`provider/types.rs:87`），无需修改。

---

### 3.3 StopReason 感知

#### 3.3.1 新增 StopReason 类型

**文件：** `src/provider/types.rs`

```rust
/// LLM 停止生成 token 的原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// 模型认为回答已完成，主动结束。
    EndTurn,
    /// 输出达到 max_tokens 上限被截断。
    MaxTokens,
    /// 遇到自定义 stop_sequence。
    StopSequence,
    /// 模型请求调用 tool。
    ToolUse,
    /// 未知原因（前向兼容）。
    #[serde(other)]
    Unknown,
}
```

`#[serde(other)]` 确保 Anthropic API 未来新增 stop_reason 值时不会导致反序列化失败。

#### 3.3.2 扩展 ProviderStreamEvent

**文件：** `src/provider/mod.rs`

```rust
pub enum ProviderStreamEvent {
    TextDelta(String),
    ToolUseStart { id: String, name: String },
    ToolUseInputDelta(String),
    ToolUseEnd,
    MessageComplete {
        usage: Usage,
        stop_reason: Option<StopReason>,  // 新增
    },
}
```

#### 3.3.3 从 Anthropic SSE 提取 stop_reason

**文件：** `src/provider/anthropic.rs`

Anthropic API 的 SSE 流中，`stop_reason` 出现在 `message_delta` 事件的 `delta` 字段中：

```json
{
  "type": "message_delta",
  "delta": { "stop_reason": "end_turn", "stop_sequence": null },
  "usage": { "output_tokens": 89 }
}
```

但当前 `MessageComplete` 是在 `MessageStop` 事件中发出的（`anthropic.rs:182-185`），此时 `stop_reason` 已经不在作用域中。

**解决方案：** 在 `AnthropicStreamReceiver` 中添加跨帧状态字段，仿照已有的 `current_tool_id` / `current_tool_name` 模式：

```rust
pub struct AnthropicStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    pending_stop_reason: Option<StopReason>,  // 新增
}
```

在 `MessageDelta` 分支中解析并暂存：

```rust
StreamEvent::MessageDelta { delta, usage } => {
    // 提取 stop_reason
    if let Some(reason_val) = delta.get("stop_reason") {
        if !reason_val.is_null() {
            if let Ok(reason) = serde_json::from_value::<StopReason>(reason_val.clone()) {
                self.pending_stop_reason = Some(reason);
            }
        }
    }
    // 文本增量仍然正常处理
    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
        return Ok(Some(ProviderStreamEvent::TextDelta(text.to_string())));
    }
    continue;
}
```

在 `MessageStop` 分支中带出：

```rust
StreamEvent::MessageStop { usage } => {
    ProviderStreamEvent::MessageComplete {
        usage: usage.unwrap_or_default(),
        stop_reason: self.pending_stop_reason.take(),
    }
}
```

---

### 3.4 MaxTokens 自动续写

**问题根因：** 当 LLM 输出达到 `max_tokens` 限制时，响应被截断。当前 agent 将截断的文本视为正常完成，导致回答不完整。

**解决方案：** 在每次迭代的流接收结束后，检查 `stop_reason`。如果为 `MaxTokens` 且无 tool call，注入一条 continuation 消息并重新进入循环：

```rust
let mut last_stop_reason: Option<StopReason> = None;

// MessageComplete 匹配分支中：
ProviderStreamEvent::MessageComplete { usage, stop_reason } => {
    last_stop_reason = stop_reason;
    // ... 累计用量 ...
}

// 流接收结束后，tool_calls 检查之前：
if last_stop_reason == Some(StopReason::MaxTokens) && tool_calls.is_empty() {
    // 已截断的 assistant 消息已追加到 all_messages
    // 注入 continuation 请求
    all_messages.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "Please continue from where you left off.".to_string(),
        }],
    });
    // 不追加到 turn_messages（这是内部实现细节）
    continue; // 回到循环顶部
}
```

**约束：**
- 自动续写消耗一次迭代配额，受 `max_iterations` 约束，不会无限循环。
- continuation 消息不加入 `turn_messages`，对外部调用方透明。

---

### 3.5 迭代上限通知

**问题根因：** 当 `for` 循环自然结束（达到 `max_iterations`），不发送任何事件。当前 `agent.rs:213-223` 的处理逻辑脆弱：仅在最后一次迭代时尝试发送 `TurnComplete`，且依赖 `last_usage` 是否为 `Some`。

#### 3.5.1 新增 AgentEvent 变体

**文件：** `src/event.rs`

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    TurnComplete { new_messages: Vec<Message>, usage: Usage },
    /// 达到最大迭代次数时发出，在 TurnComplete 之前。
    IterationLimitReached { iterations: usize },
    Error(anyhow::Error),
}
```

#### 3.5.2 Agent 循环重构

使用 `completed_naturally` 标志替代脆弱的 `if iteration == max_iterations - 1` 判断：

```rust
let mut completed_naturally = false;

for iteration in 0..self.config.max_iterations {
    // ... 流接收和 tool 执行 ...

    if tool_calls.is_empty() {
        completed_naturally = true;
        let _ = event_tx.send(AgentEvent::TurnComplete {
            new_messages: turn_messages.clone(),
            usage: cumulative_usage.clone(),
        }).await;
        break;
    }

    // ... tool 执行和结果追加 ...
    // 移除原有的 if iteration == max_iterations - 1 判断
}

// 循环自然结束，未正常完成
if !completed_naturally {
    let _ = event_tx.send(AgentEvent::IterationLimitReached {
        iterations: self.config.max_iterations,
    }).await;
    let _ = event_tx.send(AgentEvent::TurnComplete {
        new_messages: turn_messages.clone(),
        usage: cumulative_usage.clone(),
    }).await;
}
```

**设计决策：** `IterationLimitReached` 作为独立事件而非 `TurnComplete` 的字段，原因：
- `TurnComplete` 始终是最后一个事件，调用方逻辑不需要变更
- Gateway bridge 可以将 `IterationLimitReached` 映射为 `ChatProgress { kind: "iteration_limit" }`，前端可选择性展示警告

#### 3.5.3 Gateway Bridge 适配

**文件：** `src/gateway/bridge.rs`

```rust
AgentEvent::IterationLimitReached { iterations } => {
    MessageEnvelope::ChatProgress(ProgressEvent {
        kind: "iteration_limit".to_string(),
        session_id: Some(session_id.to_string()),
        iteration: Some(iterations as i32),
        ..Default::default()
    })
}
```

`ProgressEvent.iteration` 字段已存在于 `protocol.rs:212`，无需修改协议结构。

#### 3.5.4 CLI 适配

**文件：** `src/bin/nova_cli.rs` — `render_event` 函数新增分支：

```rust
AgentEvent::IterationLimitReached { iterations } => {
    eprintln!(
        "\n{}",
        format!("[warn] iteration limit reached ({iterations} iterations)").yellow()
    );
}
```

---

### 3.6 Tool 执行超时

**问题根因：** `tool_registry.execute()` 是一个无超时的 `async` 调用。如果 tool 内部发起的 HTTP 请求挂起（如 DNS 无法解析、目标服务器不响应），整个 agent turn 会被无限期阻塞。

#### 3.6.1 AgentConfig 扩展

**文件：** `src/agent.rs`

```rust
pub struct AgentConfig {
    pub max_iterations: usize,
    pub model_config: crate::provider::ModelConfig,
    /// 单个 tool 执行超时（秒）。None 表示无超时。
    pub tool_timeout_secs: Option<u64>,
}
```

#### 3.6.2 GatewayConfig 扩展

**文件：** `src/config.rs`

```rust
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// 单个 tool 执行超时（秒）。None 表示无超时。
    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,
}
```

`Default` impl 中默认为 `None`（向后兼容，无超时）。`config.toml` 中可选配置：

```toml
[gateway]
tool_timeout_secs = 120
```

#### 3.6.3 超时包裹

**文件：** `src/agent.rs` — tool 执行 future 内部：

```rust
let timeout_dur = self.config.tool_timeout_secs.map(std::time::Duration::from_secs);

tool_results_fut.push(async move {
    let _ = tx.send(AgentEvent::ToolStart { ... }).await;

    let result = if let Some(dur) = timeout_dur {
        match tokio::time::timeout(dur, tool_registry.execute(&name, input_val)).await {
            Ok(r) => r,
            Err(_elapsed) => Err(anyhow::anyhow!(
                "Tool '{}' timed out after {}s", name, dur.as_secs()
            )),
        }
    } else {
        tool_registry.execute(&name, input_val).await
    };

    let (content, is_error) = match result {
        Ok(out) => (out.content, out.is_error),
        Err(e) => (format!("Internal execution error: {}", e), true),
    };

    let _ = tx.send(AgentEvent::ToolEnd { ... }).await;
    (call_index, ContentBlock::ToolResult { tool_use_id: id, output: content, is_error })
});
```

超时后，tool future 被丢弃（dropped），内部的 HTTP 连接和子进程资源由 Rust 的 drop 语义清理。对于 `BashTool` 产生的子进程，后续可考虑增加 kill 逻辑，不在本次范围内。

#### 3.6.4 调用方适配

**文件：** `src/gateway/mod.rs`

```rust
let agent_config = AgentConfig {
    max_iterations: config.gateway.max_iterations,
    model_config: config.llm.model_config.clone(),
    tool_timeout_secs: config.gateway.tool_timeout_secs,
};
```

**文件：** `src/bin/nova_cli.rs`

```rust
let agent_config = AgentConfig {
    max_iterations: 5,
    model_config: config.llm.model_config.clone(),
    tool_timeout_secs: Some(120), // CLI 默认 120 秒
};
```

---

### 3.7 取消机制（CancellationToken）

#### 3.7.1 新增依赖

**文件：** `Cargo.toml`

```toml
tokio-util = { version = "0.7", features = ["sync"] }
```

`tokio-util 0.7` 与 `tokio 1` 兼容。`sync` feature 提供 `CancellationToken`。

#### 3.7.2 run_turn 签名变更

**文件：** `src/agent.rs`

```rust
use tokio_util::sync::CancellationToken;

pub async fn run_turn(
    &self,
    history: &[Message],
    user_input: &str,
    event_tx: mpsc::Sender<crate::event::AgentEvent>,
    cancel: Option<CancellationToken>,
) -> Result<Vec<Message>>
```

所有调用方传入 `None` 以保持行为不变。

#### 3.7.3 取消检查点

在两个主要阻塞点使用 `tokio::select!`：

**检查点 1 — LLM 流接收：**

```rust
loop {
    tokio::select! {
        result = receiver.next_event() => {
            match result? {
                Some(event) => { /* 正常处理 */ }
                None => break, // 流结束
            }
        }
        _ = cancel_or_pending(&cancel) => {
            let _ = event_tx.send(AgentEvent::Error(
                anyhow::anyhow!("Turn cancelled")
            )).await;
            return Ok(turn_messages);
        }
    }
}
```

**检查点 2 — Tool 执行等待：**

```rust
loop {
    tokio::select! {
        maybe_pair = tool_results_fut.next() => {
            match maybe_pair {
                Some(pair) => indexed_results.push(pair),
                None => break,
            }
        }
        _ = cancel_or_pending(&cancel) => {
            let _ = event_tx.send(AgentEvent::Error(
                anyhow::anyhow!("Turn cancelled during tool execution")
            )).await;
            return Ok(turn_messages);
        }
    }
}
```

**辅助函数：**

```rust
/// 等待 CancellationToken 被触发，若为 None 则永远挂起。
async fn cancel_or_pending(token: &Option<CancellationToken>) {
    match token {
        Some(t) => t.cancelled().await,
        None => std::future::pending::<()>().await,
    }
}
```

#### 3.7.4 调用方适配

**`src/bin/nova_cli.rs`：** CLI 当前通过 `tokio::select!` + `ctrl_c()` 处理中断（`nova_cli.rs:153-168`），传 `None` 即可，原有行为不变。未来可创建 `CancellationToken` 并在 `ctrl_c` 时 `cancel()`，实现更优雅的中止。

**`src/gateway/router.rs`：** Gateway 传 `None`。后续可在 `handle_chat` 中创建 token，并在 WebSocket 断开时触发，此为后续增强，不在本次范围。

---

## 4. 涉及文件变更清单

| 文件 | 变更类型 | 内容 |
|------|---------|------|
| `Cargo.toml` | 新增依赖 | `tokio-util = { version = "0.7", features = ["sync"] }` |
| `src/provider/types.rs` | 新增类型 | `StopReason` 枚举 |
| `src/provider/mod.rs` | 接口变更 | `MessageComplete` 增加 `stop_reason` 字段 |
| `src/provider/anthropic.rs` | 逻辑增强 | 新增 `pending_stop_reason` 字段，`MessageDelta` 解析 stop_reason |
| `src/event.rs` | 新增变体 | `IterationLimitReached { iterations: usize }` |
| `src/agent.rs` | 核心重写 | 全部 6 项改进合并实现 |
| `src/config.rs` | 配置扩展 | `GatewayConfig` 增加 `tool_timeout_secs` 字段 |
| `src/gateway/mod.rs` | 配置传递 | `AgentConfig` 构造增加 `tool_timeout_secs` |
| `src/gateway/bridge.rs` | 事件映射 | 新增 `IterationLimitReached` 分支 |
| `src/gateway/router.rs` | 签名适配 | `run_turn` 调用增加 `cancel: None` 参数 |
| `src/bin/nova_cli.rs` | 适配 | `AgentConfig` 和 `run_turn` 调用更新，`render_event` 新增分支 |

## 5. 实现顺序

各改动存在编译依赖关系，需按以下顺序实施：

```
Step 1: Cargo.toml                        (新增 tokio-util 依赖)
   │
Step 2: src/provider/types.rs             (新增 StopReason 枚举)
   │
Step 3: src/provider/mod.rs               (MessageComplete 扩展)
   │    └── 此步骤后 agent.rs 和 anthropic.rs 编译会报错，需与 Step 4/6 一起完成
   │
Step 4: src/provider/anthropic.rs         (提取 stop_reason)
   │
Step 5: src/event.rs                      (新增 IterationLimitReached)
   │
Step 6: src/agent.rs                      (核心重写，依赖 Step 1-5)
   │
Step 7: src/config.rs                     (配置扩展)
   │    src/gateway/mod.rs                (配置传递)
   │    src/gateway/bridge.rs             (新事件映射)
   │    src/gateway/router.rs             (签名适配)
   │    src/bin/nova_cli.rs               (CLI 适配)
   │    └── 这些可并行修改，完成后一起编译
   │
Step 8: cargo build --features gateway,cli && cargo test
```

**原子性要求：** Step 3 + Step 4 + Step 6 必须在同一次编译中完成（`MessageComplete` 签名变更会同时影响 provider 和 consumer 侧）。

## 6. 向后兼容性分析

| 变更点 | 兼容性 | 说明 |
|--------|--------|------|
| `AgentConfig` 新增字段 | ⚠️ 结构体字面量破坏 | 所有构造 `AgentConfig` 的代码需添加 `tool_timeout_secs` 字段。crate 内部使用，无外部消费者。 |
| `run_turn` 签名变更 | ⚠️ 调用方需更新 | 新增 `cancel: Option<CancellationToken>` 参数。crate 内 3 个调用点需更新。 |
| `ProviderStreamEvent` 变更 | ⚠️ match 分支需更新 | `MessageComplete` 增加字段，已有 match 需用 `..` 或绑定新字段。仅 `agent.rs` 受影响。 |
| `AgentEvent` 新增变体 | ⚠️ 非穷举 match 可能 warn | `bridge.rs` 和 `nova_cli.rs` 的 match 需增加分支。 |
| `GatewayConfig` 新增字段 | ✅ 完全兼容 | `#[serde(default)]` 确保旧 config.toml 不报错。`Default` impl 返回 `None`。 |
| Gateway WebSocket 协议 | ✅ 完全兼容 | 仅新增 `chat.progress { kind: "iteration_limit" }` 事件，前端不识别时可安全忽略。 |

## 7. 验证方案

### 7.1 编译验证

```bash
cargo build --features gateway,cli
cargo test
```

### 7.2 功能验证

| 场景 | 验证方法 | 预期结果 |
|------|---------|---------|
| Tool 结果顺序 | 发送一条会触发多个 tool call 的消息 | `tool_result` 顺序与 `tool_use` 一致 |
| 累计用量 | 发送会触发多轮迭代的复杂任务 | `TurnComplete.usage` 包含所有迭代的累计 token |
| max_tokens 续写 | 设置极小的 `max_tokens`（如 100），发送长问题 | 自动注入 continuation，最终得到完整回答 |
| 迭代上限 | 设置 `max_iterations=2`，发送需要多次 tool 调用的任务 | 收到 `iteration_limit` progress 事件 + `TurnComplete` |
| Tool 超时 | 设置 `tool_timeout_secs=5`，调用一个会长时间阻塞的 tool | 5 秒后 tool 返回超时错误，agent 继续运行 |
| 取消 | Gateway 场景下，发送 chat 后立即断开 WebSocket | agent turn 被中止，不再消耗资源 |

## 8. 后续增强（不在本次范围）

- **Gateway 取消集成：** 在 `handle_chat` 中创建 `CancellationToken`，在 WebSocket 连接断开时触发。
- **BashTool 进程 kill：** tool 超时或取消后，主动 kill 子进程而非仅 drop future。
- **流式错误重试：** LLM stream 失败时指数退避重试（当前直接返回错误）。
- **Thinking 块支持：** 扩展 `ContentBlock` 和 `ProviderStreamEvent` 以支持 Extended Thinking。
