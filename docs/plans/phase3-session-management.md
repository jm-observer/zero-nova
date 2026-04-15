# Phase 3: Session Management

## Goal

实现内存会话存储，支持 OpenFlux 前端的会话列表、历史消息加载、新建会话等 UI 功能。

## Prerequisites

- Phase 2 完成 (WS server + router 骨架)

## Tasks

### 3.1 实现 session.rs — SessionStore

```rust
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Session>>,
}

pub struct Session {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SessionStore {
    pub fn new() -> Self;

    /// 创建新会话
    pub async fn create(&self, title: &str, agent_id: &str) -> Session;

    /// 列出所有会话 (按 updated_at 降序)
    pub async fn list(&self) -> Vec<SessionSummary>;

    /// 获取会话详情 (含消息历史)
    pub async fn get(&self, id: &str) -> Option<Session>;

    /// 获取消息历史 (只要 messages，给 run_turn 用)
    pub async fn get_messages(&self, id: &str) -> Vec<Message>;

    /// 追加消息 (user input + agent response)
    pub async fn append_messages(&self, id: &str, msgs: &[Message]);

    /// 更新标题 (可选: 从首条消息自动生成)
    pub async fn update_title(&self, id: &str, title: &str);

    /// 删除会话
    pub async fn delete(&self, id: &str) -> bool;
}

/// 列表展示用的摘要 (不含完整消息)
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 3.2 实现 session handlers in router.rs

**sessions.list**:

```rust
/// 返回格式需与 OpenFlux 前端 sessions store 对齐
/// payload: [{ id, title, agentId, messageCount, createdAt, updatedAt }]
async fn handle_sessions_list(msg, state, ws_tx) {
    let sessions = state.sessions.list().await;
    send(ws_tx, "sessions.list", msg.id, json!(sessions)).await;
}
```

**sessions.get**:

```rust
/// payload: { session: { id, title, ... }, messages: [...] }
async fn handle_session_get(msg, state, ws_tx) {
    let id = msg.payload["sessionId"].as_str();
    match state.sessions.get(id).await {
        Some(s) => send(ws_tx, "sessions.get", msg.id, json!({
            "session": { "id": s.id, "title": s.title, ... },
            "messages": convert_messages_to_frontend_format(&s.messages),
        })).await,
        None => send_error(ws_tx, msg.id, "Session not found").await,
    }
}
```

**sessions.create**:

```rust
/// payload: { title?, agentId? }
async fn handle_session_create(msg, state, ws_tx) {
    let title = msg.payload["title"].as_str().unwrap_or("New Chat");
    let session = state.sessions.create(title, "nova").await;
    send(ws_tx, "sessions.create", msg.id, json!(session)).await;
}
```

### 3.3 消息格式转换

zero-nova 的 `Message { role, content: Vec<ContentBlock> }` 需要转为前端期望的格式:

```rust
/// zero-nova Message → 前端展示格式
fn convert_messages_to_frontend_format(msgs: &[Message]) -> Vec<serde_json::Value> {
    msgs.iter().map(|m| json!({
        "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
        "content": m.content.iter().map(|block| match block {
            ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
            ContentBlock::ToolUse { id, name, input } => json!({
                "type": "tool_use", "id": id, "name": name, "input": input
            }),
            ContentBlock::ToolResult { tool_use_id, output, is_error } => json!({
                "type": "tool_result", "tool_use_id": tool_use_id,
                "output": output, "is_error": is_error
            }),
        }).collect::<Vec<_>>(),
    })).collect()
}
```

### 3.4 单元测试

- SessionStore CRUD 操作
- 并发读写安全性 (多个 tokio task 同时操作)
- 消息格式转换正确性
- 空会话 / 不存在的会话 边界处理

## New/Modified Files

```
src-tauri/src/nova_gateway/
├── session.rs    # NEW
├── router.rs     # MODIFIED: 加入 session handlers
└── mod.rs        # MODIFIED: pub mod session
```

## Definition of Done

- [ ] sessions.list 返回会话列表
- [ ] sessions.create 创建新会话
- [ ] sessions.get 返回会话详情和消息历史
- [ ] 消息格式与前端渲染器兼容
- [ ] 单元测试通过
