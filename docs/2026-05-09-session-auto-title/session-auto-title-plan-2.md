# Plan 2: 后端触发策略与生成逻辑

## 前置依赖
- Plan 1（标题生成组件建模与配置）

## 本次目标
- 实现事件驱动的标题生成触发机制
- 集成异步生成执行，不阻塞主聊天流程
- 确保并发安全和幂等性

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs` - 消息处理入口
- `crates/nova-agent/src/app/conversation_service.rs` - 会话服务
- `crates/nova-agent/src/app/session_title_service.rs` - 标题生成服务（Plan 1 新建）
- `crates/nova-agent/src/provider/mod.rs` - LLM 客户端接口

## 详细设计

### 1. 触发时机（明确为事件驱动）

**触发模式：事件驱动**

标题生成只在**用户消息到达时**触发检查，而不是每轮轮询。具体流程：

1. 用户发送消息 → 消息保存成功
2. 后端检查当前会话的 `title_generation` 状态：
   - 若 `title_generation.status == Succeeded`：跳过，不重复生成
   - 若 `title_generation.status == Failed` 且 `title_generation.attempt_count >= max_attempts`：跳过
   - 若满足触发条件：先将状态原子更新为 `Pending`，再启动异步标题生成任务
3. 标题生成任务完成后，更新 `title_generation` 状态

**触发条件细化**：
- 用户消息数 `< 2`：不触发
- 用户消息数 `>= 2` 且 `title_generation.attempt_count == 0`：首次尝试
- 用户消息数 `>= 3` 且 `title_generation.attempt_count == 1` 且 `title_generation.last_error.is_some()`：重试

### 2. 标题生成 API 设计

**API 签名**：
```rust
async fn generate_session_title(
    session_id: SessionId,
    messages: &[UserMessage],
) -> Result<String, TitleGenerationError>;
```

**输入**：
- 当前会话的所有用户消息文本（或最近 N 条）
- 建议只使用最近 3-5 条用户消息，避免过长上下文

**输出**：
- 成功：标题字符串（建议最大长度 50 字符）
- 失败：`TitleGenerationError` 枚举

**错误类型**：
```rust
enum TitleGenerationError {
    NetworkError,      // 网络错误，应该重试
    Timeout,           // 超时，应该重试
    EmptyResponse,     // 模型返回空标题，不需要重试
    InvalidResponse,   // 模型返回格式错误，不需要重试
}
```

**超时设置**：建议 5 秒超时，超时算作一次失败尝试（增加 attempt_count）。

**独立 Prompt**：
- 使用独立的轻量 prompt，例如：
  ```
  根据以下对话内容，生成一个简短的会话标题（10字以内）：
  {messages}
  ```
- 使用独立的模型配置或 endpoint，避免与主对话争抢配额

### 3. 异步生成执行

**生成任务不挂在 `start_turn` 主 future 上，使用 `tokio::spawn` 后台执行。**

**生成输入仅包含有限数量的用户消息摘要，避免把整段历史直接送入模型。**

**Prompt 约束**：
- 输出单行短标题。
- 不带引号、句号、前缀。
- 优先反映用户真实目标，而非泛化成"聊天""提问"。

**成功时**：
- 规范化标题长度，例如 `6..=40` 字符。
- 更新 `sessions.title`。
- `title_generation.source = TitleSource::Ai`，`title_generation.status = TitleStatus::Succeeded`。

**失败时**：
- 仅记录日志和 `title_generation.last_error`。
- `title_generation.status = TitleStatus::Failed`。
- 不向上抛错，不影响当前 turn。

### 4. 并发与幂等

**确保不重复生成的机制**：
1. **状态机约束**：`TitleStatus::Succeeded` 状态下不再触发生成
2. **生成中锁**：`TitleStatus::Generating` 状态下，即使收到新消息也不重复启动任务
3. **生成任务去重**：使用 `session_id + attempt_count` 作为任务唯一标识，避免并发重复

**并发安全**：
- 标题生成任务启动前，将状态从 `Pending` 更新为 `Generating`（CAS 操作）
- 如果更新失败（已被其他任务抢占），则跳过本次生成
- **实现层**：基于 `ControlState` 内存锁 + SQLite 条件更新双重保障
  - 内存层：使用 `tokio::sync::Mutex` 确保同一 session 只有一个生成任务
  - 应用层：`SessionTitleService` 内部维护 `DashMap<SessionId, Arc<tokio::sync::Mutex<()>>>` 实现 session 级锁
  - 数据库层：所有状态写回统一更新 `runtime_control` 字段，且使用条件更新保证幂等：
    `UPDATE sessions SET runtime_control = ? WHERE id = ? AND json_extract(runtime_control, '$.title_generation.status') != 'generating'`
  - Repository 层新增原子接口：`try_mark_title_generating(session_id, expected_attempt_count) -> bool`

**每个 session 只允许一个标题生成任务同时运行。**
**若第 2 条消息后已触发生成，第 3 条消息到来时：**
- 若前一任务仍 `pending`，不并发再开新任务。
- 若前一任务失败，且尚未超过最大尝试数，则允许再次触发。
**一旦 `source` 变为 `ai` 或 `manual`，后续用户消息不再自动覆盖标题。**

### 5. 降级策略

**降级路径**：
1. 标题生成连续失败 2 次后，停止尝试，保持占位标题
2. 如果标题生成完全不可用（模型服务故障），前端显示占位标题，不阻塞聊天
3. 标题生成超时算作一次失败尝试，增加 `attempt_count`，等待下次用户消息再尝试

### 6. 日志与监控

**关键日志点**：
- `INFO`：标题生成任务启动
- `INFO`：标题生成成功，新标题：{title}
- `WARN`：标题生成失败，重试次数：{attempt_count}，错误：{error}
- `ERROR`：标题生成连续失败，停止尝试
- `DEBUG`：标题生成跳过（原因：{reason}）

**监控指标**：
- 标题生成成功率
- 标题生成平均耗时
- 标题生成重试率
- 标题生成超时率

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
  - 标题状态 CAS 失败时当前请求应跳过生成，不影响 turn 主流程。
