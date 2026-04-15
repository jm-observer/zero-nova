# Phase 6: Polish & Production Ready

## Goal

前端兼容性调试、边界情况处理、日志完善，确保端到端可用。

## Prerequisites

- Phase 5 完成 (Tauri 集成可运行)
- 能在 OpenFlux 前端中正常打开聊天界面

## Tasks

### 6.1 前端兼容性调试

这是最关键的调试阶段。OpenFlux 前端的各种 UI 组件对 WebSocket 消息的字段名、结构有具体期望。

**方法**: 在浏览器 DevTools 中观察前端对收到消息的解析，逐一对齐:

- [ ] `chat.progress` 的 token 字段能被 markdown 渲染器正确消费
- [ ] `chat.progress` 的 tool 事件能被工具调用 UI 正确展示
- [ ] `chat.complete` 后消息正确添加到对话列表
- [ ] `sessions.list` 返回的格式能被侧边栏正确渲染
- [ ] `sessions.get` 的历史消息能正确回显
- [ ] `agents.list` 能在 agent 选择器中正确展示

### 6.2 消息格式微调

根据 6.1 调试结果，可能需要调整:

```rust
// bridge.rs 中可能需要的调整示例:

// 前端可能期望 progress 包含 iteration 信息
AgentEvent::TextDelta(text) => json!({
    "kind": "token",
    "token": text,
    "iteration": current_iteration,  // 可能需要新增
})

// 前端可能期望 tool 信息包含描述
AgentEvent::ToolStart { name, input, .. } => json!({
    "kind": "tool_start",
    "name": name,
    "description": format!("Calling {}", name),  // 可能需要
    "input": input,
})
```

### 6.3 并发与稳定性

- [ ] 快速连续发送多条消息: 前一条未完成时发送下一条，应排队或拒绝
- [ ] 客户端断开重连: WS server 正确清理旧连接状态
- [ ] Agent 执行超时: 设置合理的 max_iterations，避免无限循环
- [ ] 大量文本输出: 工具返回超长内容时的截断处理

```rust
// 消息排队/拒绝机制
struct ConnectionState {
    is_processing: AtomicBool,
}

async fn handle_chat(msg, state, ws_tx) {
    if state.conn.is_processing.swap(true, Ordering::SeqCst) {
        send_error(ws_tx, msg.id, "A chat request is already in progress").await;
        return;
    }
    // ... 执行 agent
    state.conn.is_processing.store(false, Ordering::SeqCst);
}
```

### 6.4 日志体系

```rust
// server.rs
log::info!("[nova-gw] WS server listening on {}:{}", host, port);
log::info!("[nova-gw] Client connected: {}", addr);
log::debug!("[nova-gw] ← recv: type={}", msg.msg_type);
log::debug!("[nova-gw] → send: type={}", response.msg_type);

// router.rs
log::info!("[nova-gw] chat start: session={}, input_len={}", session_id, input.len());
log::info!("[nova-gw] chat complete: session={}, tokens={}/{}",
    session_id, usage.input_tokens, usage.output_tokens);
log::error!("[nova-gw] chat error: session={}, err={}", session_id, e);

// bridge.rs
log::trace!("[nova-gw] bridge: {:?} → {}", event_type, msg_type);
```

### 6.5 优雅关闭增强

```rust
// 使用 CancellationToken 实现优雅关闭
use tokio_util::sync::CancellationToken;

pub struct NovaGateway {
    cancel: CancellationToken,
}

impl NovaGateway {
    pub async fn start(...) -> Self {
        let cancel = CancellationToken::new();
        let token = cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        log::info!("[nova-gw] shutting down...");
                        break;
                    }
                    result = listener.accept() => {
                        // handle connection
                    }
                }
            }
        });

        Self { cancel }
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }
}
```

### 6.6 可选: 持久化 SessionStore

如果需要重启后保留会话历史:

```rust
// 使用 SQLite (via rusqlite)
pub struct SqliteSessionStore {
    db: Connection,
}

// 或简单方案: JSON 文件
pub struct FileSessionStore {
    path: PathBuf,
}
```

MVP 阶段保持 in-memory 即可，此项标记为 optional。

## Definition of Done

- [ ] OpenFlux 前端所有 UI 功能正常工作
- [ ] 聊天流式输出平滑无卡顿
- [ ] 工具调用过程正确展示
- [ ] 会话切换和历史加载正常
- [ ] 错误情况有明确的 UI 提示
- [ ] 应用启动/关闭无异常
- [ ] 日志输出清晰可追踪
