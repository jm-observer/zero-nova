# Plan 2: OpenAI 兼容 Provider Header 注入实现

## 前置依赖
- Plan 1

## 本次目标
- 在 OpenAI 兼容出站请求中注入 `x-session-id` 与 `x-agent-id`。
- 保持流式响应解析逻辑不变，不影响现有事件生产。

## 涉及文件
- `crates/nova-agent/src/provider/openai_compat/mod.rs`
- `crates/nova-agent/src/provider/openai_compat/conv.rs`（如需补充构建参数）
- `crates/nova-agent/src/provider/health.rs`（仅在需要统一 Header 构造逻辑时）

## 详细设计

### 1. 注入点
目标注入点为 `OpenAiCompatClient::stream` 发起 HTTP 请求前。

当前实现使用 `async-openai` 客户端：
- 若 SDK 提供“附加默认 Header”能力：直接在 client config/request builder 注入。
- 若 SDK 不支持：改为该 provider 内部使用 `reqwest` 手动调用 chat/completions 流接口，仅替换请求发起层。

优先级：
1. 复用 SDK 注入 Header（最小改动）。
2. 局部改为 `reqwest`（次选，保持输出事件映射不变）。

### 2. Header 构造规则
- Header Key：
  - `x-session-id`
  - `x-agent-id`
- Header Value：
  - 来自 `ProviderRequestContext`
  - `trim` 后非空才注入
- 不注入 `null`、空串、仅空白值

### 3. 日志与可观测性
增加 debug 级别日志（不打印敏感内容）：
- `session_id` 和 `agent_id` 是否注入（布尔）
- 不记录完整消息体

示例：
- `llm_request_headers: session_id=true, agent_id=false`

### 4. 兼容策略
- 上游若忽略未知 Header：无行为变化。
- 上游若拒绝未知 Header：通过 Plan 3 开关快速关闭。

## 测试案例
1. 正常透传：请求中可观测到两个 Header。
2. 单字段透传：仅一个 Header 出现。
3. 空上下文：无新增 Header。
4. 流式输出回归：`TextDelta/ToolUse/MessageComplete` 事件序列与改造前一致。
