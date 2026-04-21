# Phase 1：运行时与协议契约收敛

> 前置依赖：无  
> 基线代码：`src/agent.rs`、`src/event.rs`、`src/gateway/protocol.rs`、`src/gateway/bridge.rs`

## 1. 目标

当前仓库已经有：

- `AgentRuntime`
- `AgentEvent`
- `GatewayMessage`
- `bridge`
- `WebSocket gateway`

所以第一阶段不再是“从 0 搭协议桥”。  
真正要做的是：**先把运行时事件、协议消息、桥接职责收敛成稳定契约**。

这个 phase 完成后，后续 agent 应能基于一套稳定接口继续开发，不需要再猜：

- `AgentEvent` 哪些事件会发
- `bridge` 是否负责发 `chat.complete`
- `router` 和 `bridge` 的边界在哪里
- `chat.progress` 的 payload 长什么样

## 2. 当前代码现状

基于 `src` 的现状，已存在几个直接问题：

1. `src/event.rs` 中 `AgentEvent::Error(anyhow::Error)` 不利于协议层和测试层复用。
2. `src/gateway/bridge.rs` 当前会直接把 `TurnComplete` 转成 `chat.complete`。
3. `src/gateway/router.rs` 在 `handle_chat()` 末尾没有统一发送最终完成消息，而是依赖 bridge。
4. `src/gateway/protocol.rs` 的消息集合非常宽，混入了大量当前后端并未真正支撑的类型。
5. `ProgressEvent` 的字段语义还不够稳定，比如：
   - `tool` 是 `name:id` 拼接字符串
   - `result` 直接塞字符串转 `Value`
   - `iteration` 仅在特定事件里使用

如果不先统一这些契约，后续做控制层、workflow、多 agent 时会持续返工。

## 3. 本 phase 范围

### 3.1 要做

- 收敛 `AgentEvent`
- 收敛 `GatewayMessage / MessageEnvelope / ProgressEvent`
- 明确 `bridge` 与 `router` 边界
- 固定 `chat.start / chat.progress / chat.complete / error` 的行为
- 补齐协议与桥接测试

### 3.2 不做

- 不改 SessionStore 设计
- 不加 workflow / skill / multi-agent
- 不做前端联调
- 不做持久化

## 4. 设计结论

### 4.1 `bridge` 只负责中间事件，不负责最终完成

建议明确职责：

- `bridge` 负责：
  - `TextDelta`
  - `ToolStart`
  - `ToolEnd`
  - `IterationLimitReached`
  - `Error`
- `router` 负责：
  - `chat.start`
  - `chat.complete`
  - `通用 error response`

原因：

- `router` 才知道 session 是否已成功写入
- `router` 才知道最终返回里要不要附加 `session_id` / `usage`
- 避免 `TurnComplete -> chat.complete` 与主流程双发

### 4.2 协议先收敛到当前后端真实支持的子集

`src/gateway/protocol.rs` 里当前消息很多，但后端真实可实现的只有一部分。  
第一阶段建议先把文档和代码都聚焦到以下稳定子集：

- `welcome`
- `auth`
- `chat`
- `chat.start`
- `chat.progress`
- `chat.complete`
- `error`
- `sessions.list`
- `sessions.messages`
- `sessions.create`
- `agents.list`
- `agents.switch`

不是说要删掉其他枚举，而是本阶段设计和测试只围绕**真实可支撑子集**展开。

### 4.3 `ProgressEvent` 字段要语义化

当前 `tool = "name:id"` 太脆弱。建议改成：

- `tool_name`
- `tool_use_id`
- `args`
- `result`
- `is_error`

如果暂时不改协议字段，也要在 phase 文档里明确这是技术债，后续 phase 统一替换。

## 5. 实现细节

### 5.1 收敛 `AgentEvent`

建议演进为：

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    IterationLimitReached { iterations: usize },
    Error(String),
}
```

`TurnComplete` 不一定要删，但建议逐步退出事件桥接路径，改由 `router` 在 `run_turn()` 成功返回后统一发 `chat.complete`。

### 5.2 收敛 `bridge`

目标函数保留：

```rust
pub fn agent_event_to_gateway(
    event: AgentEvent,
    request_id: &str,
    session_id: &str,
) -> GatewayMessage
```

但行为要改：

- 不再输出 `ChatComplete`
- `Error` 统一转 `MessageEnvelope::Error`
- `ToolEnd` 要保留 `is_error`

### 5.3 协议测试固定 JSON 形状

测试不能只断 Rust 枚举匹配，要直接断序列化结果，例如：

- `chat.progress` token 事件
- `tool_start`
- `tool_result`
- `error`

## 6. 实施步骤

### Step 1：梳理协议子集

文件：

- `src/gateway/protocol.rs`

动作：

- 标记当前真实支持的消息类型
- 明确关键 payload 字段
- 为后续 phase 保留兼容空间

### Step 2：调整 `AgentEvent`

文件：

- `src/event.rs`
- `src/agent.rs`

动作：

- 让错误事件更适合传输和测试
- 去掉或下沉 `TurnComplete` 在桥接链路中的职责

### Step 3：调整 `bridge`

文件：

- `src/gateway/bridge.rs`

动作：

- 固定 `progress` 映射
- 不再输出最终 complete

### Step 4：补测试

文件：

- `src/gateway/protocol.rs`
- `src/gateway/bridge.rs`

动作：

- 增加协议与桥接测试

## 7. 测试方案

### 7.1 单元测试

至少覆盖：

- `GatewayMessage` 序列化/反序列化
- 每种 `AgentEvent` 的桥接输出
- `Error` 映射
- `ToolEnd.is_error` 保留

### 7.2 回归要求

命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 8. 完成定义

- `bridge` 与 `router` 职责已固定
- `chat.complete` 只有一条发送路径
- 协议子集已明确
- 协议/桥接测试通过

## 9. 给下一阶段的交接信息

Phase 2 不再讨论协议长什么样，而是在这套稳定契约上整理网关和会话主链路。
