# Design Doc: Plan B - Protocol & Error Handling (通信协议与错误处理增强)

**Date**: 2026-04-25
**Status**: Draft
**Author**: Claude Code

## 1. Current State (现状)

当前的 Zero-Nova 通信协议在异常处理和错误反馈方面存在明显的“黑盒”现象：

### 1.1 错误信息模糊
后端 `router.rs` 在处理未实现的指令时，直接返回字符串 `"Not implemented"`。对于前端而言，这仅仅是一个通用的错误字符串，无法区分是“指令不支持”、“权限不足”还是“系统繁忙”。

### 1.2 缺乏结构化错误协议
目前的 `nova-protocol` 主要围绕成功的业务逻辑（如 `Chat`, `SessionsList`）设计。当发生错误时，通常是靠在原有的 Envelope 中包装错误字符串，或者在 `GatewayMessage` 的错误分支中返回简单的错误描述。这种方式缺乏错误码（Error Code）的概念，导致前端无法实现差异化的 UI 反馈（例如：网络错误显示重试按钮，权限错误显示登录弹窗）。

### 1.3 前端感知能力弱
由于协议中缺乏显式的错误类型定义，前端 `gateway-client.ts` 只能进行“广义”的错误捕获。当后端发生逻辑错误时，前端往往只能通过显示一个通用的错误提示框来告知用户，无法进行针对性的用户引导。

---

## 2. Goals (目标)

1.  **引入结构化错误协议**: 在 `nova-protocol` 中定义标准化的错误模型。
2.  **实现错误码体系**: 建立一套涵盖应用层、协议层和系统层的错误码规范。
3.  **增强后端反馈精度**: 后端组件（Router, Handlers, Core）能够返回包含错误码和上下文信息的结构化错误。
4.  **提升前端响应能力**: 前端能够根据不同的错误码，触发差异化的 UI 交互（Toast, Modal, Retry, Login）。

---

## 3. Detailed Design (实现细节)

### 3.1 扩展 `nova-protocol`

我们需要在 `nova-protocol` 中引入一个强类型的错误结构。

#### 3.1.1 定义 `ErrorCode` 枚举
```rust
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // Protocol Errors
    Unknown = 1000,
    NotImplemented = 1001,
    InvalidPayload = 1002,

    // Application/Agent Errors
    AgentBusy = 2000,
    ToolExecutionFailed = 2001,
    MaxIterationsReached = 2002,
    LlmProviderError = 2003,

    // System/Auth Errors
    Unauthorized = 3000,
    InternalServerError = 3001,
    ConnectionLost = 3002,
}
```

#### 3.1.2 定义 `ErrorPayload`
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
    pub context: Option<serde_json::Value>, // 用于存放额外的诊断信息
}
```

#### 3.1.3 更新 `MessageEnvelope`
在 `MessageEnvelope` 中增加一个专门的 `Error` 变体：
```rust
pub enum MessageEnvelope {
    // ... 其他变体
    Error(ErrorPayload),
}
```

### 3.2 后端实现逻辑

#### 3.2.1 Router 层改进
在 `router.rs` 的 `dispatch` 函数中，不再返回原始字符串，而是构造 `MessageEnvelope::Error`。
```rust
// 示例
match envelope {
    MessageEnvelope::SomeNewCommand => {
        let error = ErrorPayload {
            code: ErrorCode::NotImplemented,
            message: "The requested command is not yet supported.".into(),
            context: None,
        };
        return Ok(MessageEnvelope::Error(error));
    }
    // ...
}
```

#### 3.2.2 Handler 层改进
`chat_handler` 在遇到 LLM 错误或工具执行严重错误时，应封装为对应的 `ErrorCode`。

### 3.3 前端实现逻辑

#### 3.3.1 `gateway-client.ts` 增强
更新客户端的解析逻辑，识别 `MessageEnvelope::Error` 变体。

#### 3.3.2 错误分发中心 (Error Dispatcher)
在前端建立一个错误处理逻辑，根据 `ErrorCode` 执行不同策略：
- `ErrorCode::Unauthorized` $\rightarrow$ 触发 `auth-store` 强制跳转登录。
- `ErrorCode::ToolExecutionFailed` $\rightarrow$ 在聊天窗口显示带“重试”按钮的错误卡片。
- `ErrorCode::NotImplemented` $\rightarrow$ 显示一个轻量级的 Toast 提示。
- `ErrorCode::InternalServerError` $\rightarrow$ 记录日志并提示用户联系开发者。

---

## 4. Test Plan (测试计划)

### 4.1 协议兼容性测试 (Breaking Change Check)
由于修改了核心协议，必须验证现有的 `nova_cli` (Stdio) 和 `deskapp` (WebSocket) 在收到新格式 `Error` 消息时的行为。

### 4.2 后端集成测试
- **模拟未实现指令**: 调用一个不存在的指令，验证返回的 JSON 是否符合 `MessageEnvelope::Error` 结构且包含正确的 `ErrorCode::NotImplemented`。
- **模拟 LLM 故障**: 在测试环境中模拟 LLM API 返回 500 错误，验证后端是否能正确包装为 `ErrorCode::LlmProviderError`。

### 4.3 前端 UI 测试
- **错误反馈测试**: 编写自动化测试或手动测试，确保不同错误码触发了正确的 UI 组件（Toast vs Modal）。
- **异常流测试**: 模拟 WebSocket 断开，验证前端是否能识别出 `ConnectionLost` 并显示断线重连提示。
