# Plan 1: 会话标题状态建模与后端生成策略

## 前置依赖
- 无

## 本次目标
- 为 session 增加可持久化的标题生成状态。
- 在不阻塞主聊天流程的前提下，定义“何时尝试、何时放弃、何时重试”的后台策略。
- 为后续网关推送和前端刷新提供稳定的数据来源。

## 涉及文件
- `crates/nova-agent/src/conversation/control.rs`
- `crates/nova-agent/src/conversation/session.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/repository.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- 可能新增：`crates/nova-agent/src/app/session_title_service.rs`

## 详细设计
### 1. 标题状态模型
- 在 `ControlState` 中新增 `title_state`，避免改动 SQLite 表结构。
- `title_state` 建议字段：
  - `source: "default" | "ai" | "manual"`
  - `status: "idle" | "pending" | "succeeded" | "failed"`
  - `attempt_count: u8`
  - `last_attempt_at: i64`
  - `last_success_at: Option<i64>`
  - `last_error: Option<String>`
  - `based_on_user_message_count: usize`
- `sessions.title` 仍保留为当前展示标题；`title_state` 负责解释这个标题是默认值还是 AI 结果。

### 2. 默认标题与创建行为
- `create_session` 不再把用户首条消息内容作为标题。
- 后端创建时统一落默认标题：
  - UI 展示统一、可国际化。
  - 避免“无信息短句”提前固化为标题。
- 默认标题建议继续作为 `sessions.title` 持久化值，而不是 `NULL`：
  - 当前 `Session.name` 为 `String`，不必为了该需求把全链路改成 `Option<String>`。
  - 复制会话、排序、旧接口兼容成本更低。

### 3. 触发条件
- 只统计 `Role::User` 且 `ContentBlock::Text` 的非空文本。
- 引入具名常量：
  - `TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT = 2`
  - `TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT = 3`
  - `TITLE_MAX_ATTEMPTS = 2`
  - `TITLE_MIN_TOTAL_CHARS = 24` 或等价配置项
- `maybe_schedule_title_generation(session_id)` 在每次用户消息入库后评估：
  - `source != default`：直接跳过。
  - `status == pending`：跳过，防止并发重复生成。
  - `attempt_count >= TITLE_MAX_ATTEMPTS`：跳过。
  - 用户消息不足门槛或有效字符不足：跳过。
  - 其余情况：切换为 `pending`，异步启动后台生成任务。

### 4. 异步生成执行
- 生成任务不挂在 `start_turn` 主 future 上，使用 `tokio::spawn` 后台执行。
- 生成输入仅包含有限数量的用户消息摘要，避免把整段历史直接送入模型。
- Prompt 约束：
  - 输出单行短标题。
  - 不带引号、句号、前缀。
  - 优先反映用户真实目标，而非泛化成“聊天”“提问”。
- 成功时：
  - 规范化标题长度，例如 `6..=40` 字符。
  - 更新 `sessions.title`。
  - `title_state.source = "ai"`，`status = "succeeded"`。
- 失败时：
  - 仅记录日志和 `title_state.last_error`。
  - `status = "failed"`。
  - 不向上抛错，不影响当前 turn。

### 5. 并发与幂等
- 每个 session 只允许一个标题生成任务同时运行。
- 若第 2 条消息后已触发生成，第 3 条消息到来时：
  - 若前一任务仍 `pending`，不并发再开新任务。
  - 若前一任务失败，且尚未超过最大尝试数，则允许再次触发。
- 一旦 `source` 变为 `ai` 或 `manual`，后续用户消息不再自动覆盖标题。

## 测试案例
- 正常路径：
  - 第 1 条用户消息后不触发生成。
  - 第 2 条用户消息后满足字数门槛，触发一次生成并成功写回标题。
- 边界条件：
  - 第 2 条消息总字符数仍过短，不触发；第 3 条补足后触发。
  - 同一 session 快速连续发消息时，只保留一个 `pending` 标题任务。
- 异常路径：
  - 模型调用失败时，聊天主流程仍成功。
  - 首次失败后第 3 条消息触发第二次尝试；第二次失败后停止重试。
  - 旧 `runtime_control` JSON 缺少 `title_state` 时能正常反序列化。
