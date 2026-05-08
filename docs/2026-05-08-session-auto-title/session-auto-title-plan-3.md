# Plan 3: 测试补充与回归验证

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 为自动标题需求补足后端单测、集成测试与前端状态测试。
- 确保“标题生成失败不影响聊天”和“延迟生成后 UI 可见”两个核心承诺可回归验证。

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/tests/integration/*`
- `deskapp/src/__tests__/chat-service.test.ts`
- `deskapp/src/__tests__/gateway-client-contract.test.ts`
- `deskapp/e2e/tests/sessions.e2e.spec.ts`

## 详细设计
### 1. 后端单元测试
- 覆盖标题状态机：
  - 默认标题 session 初始为 `idle + source=default`。
  - 第 1 条消息不触发。
  - 第 2 条消息触发 `pending`。
  - 成功后转 `succeeded`，不会再次触发。
  - 失败后记录 `attempt_count` 和 `last_error`。

### 2. 后端集成测试
- 构造 fake title generator：
  - 成功模式：断言第二或第三条用户消息后标题被写回 repository。
  - 失败模式：断言 `start_turn` 仍返回成功，session 标题维持默认值。
- 验证并发：
  - 快速连续两次消息只会产生一次并发中的标题任务。

### 3. 前端测试
- `ChatService`：
  - 首条消息建 session 时不再使用用户输入截断标题。
- `GatewayClient`：
  - 能解析并分发新的 session summary 更新事件。
- `state / view`：
  - session 标题更新后 sidebar 与 chat header 均反映新值。

### 4. E2E 回归
- 新会话进入时显示默认标题。
- 连续发送 2 到 3 条消息后，标题自动变为 AI 结果。
- 标题生成失败场景下，聊天功能继续可用，默认标题保留。

## 测试案例
- 正常路径：
  - 会话从 `New Chat` 自动切换为语义标题。
- 边界条件：
  - 短消息如“你好”“继续”不应过早触发标题。
  - 成功生成后继续聊天，标题保持稳定。
- 异常路径：
  - 生成器抛错不影响消息展示、turn 完成事件和后续对话。
  - 网关事件晚到或重复到达时，UI 最终状态仍正确。
