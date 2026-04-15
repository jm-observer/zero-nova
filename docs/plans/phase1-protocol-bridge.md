# Phase 1: Foundation — Protocol & Bridge

## Goal

建立协议类型定义和核心转换层，可单独编译和单元测试，不依赖 WebSocket 和 Tauri。

## Prerequisites

- zero-nova crate 可正常编译
- 理解 OpenFlux 前端 `gateway-client.ts` 的消息格式

## Tasks

### 1.1 修改 zero-nova: AgentEvent 加 Clone

**File**: `src/event.rs`

AgentEvent 当前只有 `#[derive(Debug)]`。Bridge 层需要 clone event 进行转换。

```diff
- #[derive(Debug)]
+ #[derive(Debug, Clone)]
  pub enum AgentEvent {
```

注意: `Error(anyhow::Error)` variant 不支持 Clone。需要改为:

```rust
#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    TurnComplete { new_messages: Vec<Message>, usage: Usage },
    Error(String),  // 改为 String，由调用方 .to_string() 转换
}
```

需要同步修改 `src/agent.rs` 中 emit Error 的地方。

### 1.2 创建模块结构

```
src-tauri/src/nova_gateway/
├── mod.rs          # pub mod 声明
├── protocol.rs     # 协议类型
└── bridge.rs       # AgentEvent → GatewayMessage
```

### 1.3 实现 protocol.rs

定义与 OpenFlux 前端完全兼容的消息类型:

```rust
/// 核心消息结构 (对应 gateway-client.ts 的 GatewayMessage)
#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}
```

包含辅助构造方法: `GatewayMessage::new()`, `GatewayMessage::error()` 等。

### 1.4 实现 bridge.rs

核心映射逻辑:

| AgentEvent           | → msg_type         | → payload.kind  |
|----------------------|--------------------|-----------------|
| `TextDelta(text)`    | `chat.progress`    | `token`         |
| `ToolStart{..}`      | `chat.progress`    | `tool_start`    |
| `ToolEnd{..}`        | `chat.progress`    | `tool_end`      |
| `TurnComplete{..}`   | `chat.complete`    | -               |
| `Error(msg)`         | `chat.error`       | -               |

实现 `fn agent_event_to_gateway_msg(event: &AgentEvent, request_id: &str) -> GatewayMessage`

### 1.5 单元测试

- 每个 AgentEvent variant 的转换输出是否符合预期 JSON 结构
- GatewayMessage 的序列化/反序列化 round-trip
- request_id 正确传递

## Definition of Done

- [ ] `cargo build` 通过
- [ ] `cargo test` bridge 相关测试全部通过
- [ ] 转换输出的 JSON 格式与 OpenFlux 前端 `gateway-client.ts` 中的消息解析逻辑兼容
