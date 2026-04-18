# Phase 3：会话模型与聊天生命周期修正

> 前置依赖：Phase 2  
> 基线代码：`src/gateway/session.rs`、`src/gateway/router.rs`、`src/agent.rs`

## 1. 目标

当前系统已经能跑 chat，但 `session` 和 `chat` 生命周期仍然比较粗糙。  
第三阶段要解决的是：**让“消息写入顺序、历史读取、并发约束、完成态”一致化。**

## 2. 当前问题

根据现有 `src`：

1. `SessionStore` 只有：
   - `create`
   - `create_with_id`
   - `get`
   - `get_all`
2. `Session` 只有：
   - `id`
   - `name`
   - `history`
   - `created_at`
   - `chat_lock`
3. `handle_chat()` 当前是在 `run_turn()` 成功后才把 user message 写入 `history`。
4. session 没有 `updated_at`
5. session 没有统一的消息追加 API
6. `sessions.messages` 直接 `serde_json::to_value(m).unwrap()`，对外格式不稳定

这会导致：

- 失败请求丢失 user message
- 历史和真实会话状态不一致
- 会话列表缺少排序依据

## 3. 本 phase 范围

### 3.1 要做

- 重做 `SessionStore` API
- 修正 `handle_chat()` 的读写顺序
- 稳定会话消息输出格式
- 引入 `updated_at`
- 明确 session 级串行策略

### 3.2 不做

- 不做持久化数据库
- 不做 history summary
- 不做 workflow / agent context

## 4. 设计结论

### 4.1 `SessionStore` 必须负责消息追加

不要让 `router` 直接抓 `RwLock<Vec<Message>>` 然后自己 push。  
建议新增 API：

- `append_user_text(...)`
- `append_messages(...)`
- `list_summaries(...)`
- `get_history(...)`

让 session 的时间戳和并发边界都由 store 负责。

### 4.2 user message 必须先落库，再执行 runtime

建议顺序：

1. 校验/创建 session
2. 获取 session lock
3. 追加 user message
4. 读取完整 history 作为 runtime 输入
5. 执行 `run_turn()`
6. 追加 assistant / tool messages
7. 发送 `chat.complete`

这样即使 LLM 失败，也能保留用户输入。

### 4.3 会话对外格式单独转换

不能继续直接 `serde_json::to_value(Message)`。  
要单独定义转换函数，把内部 `Message` 变成稳定对外结构。

## 5. 实现细节

### 5.1 扩展 `Session`

建议至少增加：

```rust
pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64 or RwLock<i64>,
    pub chat_lock: Mutex<()>,
}
```

### 5.2 扩展 `SessionStore`

建议新增方法：

- `list()`
- `append_messages(id, msgs)`
- `append_user_text(id, text)`
- `delete(id)`

### 5.3 重写 `handle_chat()` 的持久化顺序

当前代码里：

- 先取历史
- 再 `run_turn()`
- 成功后才写 user message

这个顺序必须修正。

### 5.4 稳定 `sessions.list` / `sessions.messages`

要求：

- `sessions.list` 使用 `updated_at` 排序
- `sessions.messages` 使用稳定 DTO，不直接透出内部序列化结果

## 6. 测试方案

### 6.1 SessionStore 单元测试

覆盖：

- 创建
- 获取
- 列表排序
- 追加消息
- 删除

### 6.2 Chat 生命周期测试

覆盖：

- 成功对话
- 失败对话仍保留 user message
- 同 session 并发聊天被串行化

## 7. 风险点

### 7.1 继续在 router 内直接写 `session.history`

这是后续演进的大障碍，必须收口。

### 7.2 用 `serde_json::to_value(Message)` 充当 API contract

这会让协议被内部结构绑死。

## 8. 完成定义

- SessionStore 已有完整基本 API
- `handle_chat()` 写入顺序已修正
- user message 不再因失败丢失
- `sessions.*` 输出稳定

## 9. 给下一阶段的交接信息

Phase 4 会在稳定的 session/chat 生命周期之上，引入会话控制层骨架，而不是先碰控制逻辑。
