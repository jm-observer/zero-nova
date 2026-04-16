# Gateway Protocol Implementation Design

This document outlines the technical design for aligning the Rust implementation of the Gateway Protocol in `src/gateway/protocol.rs` with the formal specification in `docs/gateway-protocol.md`.

## 1. Objective
## 1. 核心设计原则

### 1.1 协议对齐标准
- **命名规范**：JSON Payload 统一使用 `camelCase`。
- **消息结构**：
  - `id`: `Option<String>`。客户端请求必带，服务器事件通知可选。
  - `type`: 消息类别标签，如 `chat.progress`。
  - `payload`: 具体的业务数据。

### 1.2 关键重构设计
- **Payload 结构体模式 (推荐)**：
  > [!IMPORTANT]
  > **Serde 限制**：在 Enum 上直接使用 `#[serde(rename_all = "camelCase")]` 只能重构变体名称，不能对齐变体内“命名字段”的命名。
  - **规范**：禁止使用“命名字段变体”（如 `Variant { msg_id: String }`）。所有包含字段的变体必须定义为独立的结构体（如 `Variant(VariantPayload)`），并在结构体上应用 `rename_all`。
- **单元变体 (Unit Variants)**：
  - 对于无 Payload 的命令（如 `sessions.list`），变体不再包含空字段，序列化结果不包含 `payload` 属性。

## 2. 协议类别映射 (Gap Analysis)

| 类别 | 状态 | 指令示例 | 映射逻辑 |
| :--- | :--- | :--- | :--- |
| **基础控制** | ✅ 100% | `welcome`, `auth`, `error` | `GatewayMessage::new_event` |
| **会话管理** | ✅ 100% | `sessions.list`, `sessions.messages`, `sessions.logs`, `sessions.artifacts` | 通过 `SessionStore` 检索并映射 |
| **对话交互** | ✅ 100% | `chat`, `chat.start`, `chat.progress`, `chat.complete` | 桥接 `AgentEvent` -> `ProgressEvent` |
| **Agent 管理** | ✅ 100% | `agents.list`, `agents.switch` | 动态路由至不同的 Agent 运行时 |
| **系统配置** | ✅ 100% | `config.get`, `settings.get`, `settings.update`, `language.update` | 读取/更新全局配置 |
| **集成插件** | ✅ 100% | `browser.status`, `router.status`, `weixin.status`, `voice.get-status` | 获取子系统连接状态 |
| **云端同步** | ✅ 100% | `openflux.status` | 返回云端登录限制状态 |

## 3. 实现细节

### 3.1 桥接逻辑 (Bridge)
- **Flattened Events**: 将后端复杂的 `AgentEvent`（如文本片段、工具调用开始/结束）映射为协议定义的扁平化 `ProgressEvent`。
- **Usage Tracking**: 在 `chat.complete` 阶段合并 Token 统计。

### 3.2 路由分发 (Router)
- **Mock Handlers**: 为了保证前端流程畅顺，对于尚未完全实现的系统指令（如 `voice`），网关提供默认的 Mock 成功响应。
- **Optional IDs**: 路由逻辑必须能够容忍无 ID 的消息（直接丢弃或记录警告），不影响主流程。

---

## 4. 故障排除

### 4.1 常见的反序列化失效
- **错误信息**：`missing field "agent_id"` (即便已设置全局重命名)。
- **排查方向**：检查是否使用了“命名字段变体”。
- **解决**：将其提取为独立结构体，代码示例：
  ```rust
  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase")]
  struct MyPayload { agent_id: String } // 正确映射为 agentId
  ```

### 4.2 GatewayMessage 结构定义
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}
### 3.2 MessageEnvelope (Polymorphism)
We use the `tag = "type", content = "payload"` attribute.
- **Commands without payloads** (e.g., `sessions.list`) -> Unit Variant: `ListSessions`
- **Commands with payloads** -> Tuple/Struct Variant: `Chat(ChatPayload)`

**Crucial Change**: All Payload structs will use `#[serde(rename_all = "camelCase")]` to match the frontend, eliminating the need for manual `#[serde(alias = "...")]`.

### 3.3 Core Payload Redesign
#### ProgressEvent (Section 4.2 Alignment)
The current `ProgressType` will be flattened or restructured to match the specification:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String, // 'thinking' | 'tool_start' | ...
    pub iteration: Option<i32>,
    pub tool: Option<String>,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub thinking: Option<String>,
    pub token: Option<String>,
    pub output: Option<String>,
    pub session_id: Option<String>,
}
```

## 4. Implementation Detailed Mapping

### 4.1 Scheduler payload mapping
- `scheduler.list` -> `SchedulerList` (Unit)
- `scheduler.trigger` -> `SchedulerTrigger { taskId: String }`

### 4.2 Memory payload mapping
- `memory.stats` -> `MemoryStats` (Unit)
- `memory.search` -> `MemorySearch { query: String, limit: usize }`

### 4.3 Integration mapping
- `router.qr_bind_code` -> `RouterQrBindCode { status, qr_data, expires_in, ... }`

## 5. Implementation Steps
1.  **Refactor Main Structures**: Apply `Option<id>` and `flatten` envelope.
2.  **Apply CamelCase Convention**: Add `rename_all = "camelCase"` to all relevant structs/enums.
3.  **Implement All Variants**: Fill in `MessageEnvelope` with all types from `gateway-protocol.md`.
4.  **Define Sub-Structs**: Create the necessary payload structs for each new variant.
5.  **Test Suite**: Verify serialization/deserialization for each category.

## 6. Verification
- `cargo test` for serialization format.
- Ensure `{"type": "welcome"}` (no ID, no payload) parses successfully.
- Ensure `{"id": "1", "type": "chat", "payload": {"message": "hi"}}` parses correctly.
