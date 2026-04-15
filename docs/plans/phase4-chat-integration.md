# Phase 4: Chat Integration

## Goal

替换 Phase 2 的 chat stub，接入真实的 zero-nova AgentRuntime，实现完整的流式聊天功能。这是整个集成的核心阶段。

## Prerequisites

- Phase 1-3 全部完成
- ANTHROPIC_API_KEY 环境变量可用

## Tasks

### 4.1 实现 handle_chat — 完整版

核心流程:

```
Client                      Router                    Bridge / Agent
  │                            │                           │
  │─── {type:"chat"} ────────►│                           │
  │                            │ 1. parse payload          │
  │                            │ 2. get/create session     │
  │                            │ 3. load history           │
  │◄── {type:"chat.start"} ───│                           │
  │                            │ 4. create mpsc channel    │
  │                            │ 5. spawn forwarder task ─►│
  │                            │ 6. agent.run_turn() ─────►│
  │                            │                           │── LLM call
  │                            │              TextDelta ◄──│
  │◄── {chat.progress/token} ──│◄─── bridge convert ──────│
  │                            │              ToolStart ◄──│
  │◄── {chat.progress/tool} ───│◄─── bridge convert ──────│
  │                            │              ToolEnd   ◄──│
  │◄── {chat.progress/tool} ───│◄─── bridge convert ──────│
  │                            │           TurnComplete ◄──│
  │◄── {type:"chat.complete"} ─│ 7. save messages          │
  │                            │ 8. update session title   │
  │                            │                           │
```

### 4.2 详细实现

```rust
async fn handle_chat(
    msg: GatewayMessage,
    state: &Arc<GatewayState>,
    ws_tx: &WsSender,
) {
    let request_id = msg.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let payload = msg.payload.unwrap_or(json!({}));

    let input = payload["input"].as_str().unwrap_or("").to_string();
    let session_id = payload["sessionId"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // 无 sessionId 时自动创建
            // 注意: 实际需要 async，这里简化表示
            Uuid::new_v4().to_string()
        });

    // 1. 加载历史
    let history = state.sessions.get_messages(&session_id).await;

    // 2. 添加 user message 到 session
    let user_msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: input.clone() }],
    };
    state.sessions.append_messages(&session_id, &[user_msg]).await;

    // 3. 通知前端开始
    send(ws_tx, GatewayMessage::new("chat.start", &request_id, json!({}))).await;

    // 4. 创建事件通道
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);

    // 5. 启动转发任务: AgentEvent → WS
    let ws_tx_clone = ws_tx.clone();
    let rid = request_id.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let ws_msg = bridge::agent_event_to_gateway_msg(&event, &rid);
            // TurnComplete 不在这里发，由主流程发 chat.complete
            if ws_msg.msg_type != "chat.complete" {
                let _ = ws_tx_clone.send(ws_msg).await;
            }
        }
    });

    // 6. 执行 Agent
    let result = {
        let agent = state.agent.lock().await;
        agent.run_turn(&history, &input, event_tx).await
    };

    // 等待转发任务完成
    let _ = forwarder.await;

    // 7. 处理结果
    match result {
        Ok(new_msgs) => {
            // 保存 assistant 消息到 session
            state.sessions.append_messages(&session_id, &new_msgs).await;

            // 提取纯文本作为 output
            let output = extract_assistant_text(&new_msgs);

            send(ws_tx, GatewayMessage::new("chat.complete", &request_id, json!({
                "output": output,
                "sessionId": session_id,
            }))).await;
        }
        Err(e) => {
            send(ws_tx, GatewayMessage::new("chat.error", &request_id, json!({
                "message": e.to_string(),
            }))).await;
        }
    }
}
```

### 4.3 辅助函数

```rust
/// 从 assistant 消息中提取纯文本
fn extract_assistant_text(msgs: &[Message]) -> String {
    msgs.iter()
        .filter(|m| m.role == Role::Assistant)
        .flat_map(|m| m.content.iter())
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
```

### 4.4 并发安全考虑

当前 AgentRuntime 通过 `Mutex<AgentRuntime>` 包装:
- 同一时间只有一个 chat 请求可以执行 (agent.lock)
- 如果需要并发，后续可以改为每次 clone AgentRuntime 或使用 agent pool

对于 MVP 阶段，单一 Mutex 足够 (桌面应用通常只有一个用户)。

### 4.5 Chat Progress 格式对齐

需要确认 OpenFlux 前端 `onProgress` handler 期望的 payload 字段:

```typescript
// gateway-client.ts 中的 progress 处理
// 需要验证前端解析的具体字段名
onProgress(handler: (event: ProgressEvent) => void)
```

可能需要微调 bridge.rs 的输出格式以匹配前端期望。这是集成中最可能需要调试的地方。

### 4.6 端到端测试

1. **纯文本对话**: 发送简单问题 → 收到 token 流 → complete
2. **工具调用**: 发送需要调用 bash/file 的指令 → 收到 tool_start/tool_end → 最终回复
3. **多轮对话**: 在同一 session 中连续发送多条消息 → 验证历史上下文正确传递
4. **错误处理**: 无效 API key → 收到 chat.error
5. **长时间执行**: 多轮工具调用 → 所有 progress 事件都正确传递

## Modified Files

```
src-tauri/src/nova_gateway/
├── router.rs     # MODIFIED: handle_chat 从 stub 替换为真实实现
├── bridge.rs     # MODIFIED: 可能微调 progress payload 格式
└── session.rs    # MODIFIED: 可能需要调整 append 逻辑
```

## Definition of Done

- [ ] 发送 chat 消息能触发 zero-nova agent 执行
- [ ] TextDelta 流式传递到前端 (chat.progress/token)
- [ ] 工具调用正确展示 (tool_start → tool_end)
- [ ] 对话历史正确保存和加载
- [ ] 多轮对话上下文连贯
- [ ] 错误情况正确返回 chat.error
