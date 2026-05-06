# Plan 1：前端状态管理

**前置依赖**: 无

---

## 本次目标

在 `ChatView` 内建立以会话为粒度的流式状态机，实现：

1. 消息发出时将当前会话标记为 streaming
2. `chat:complete` / `chat:error` 时清除 streaming 标记
3. 按钮根据当前会话的 streaming 状态在「发送」和「停止」之间切换
4. 点击停止按钮调用 `gateway.stopTask(sessionId)` 并进入「停止中」过渡态，防止重复点击
5. 接收到 `chat.stop.response`（或 `chat:complete`）时退出停止中状态
6. 切换会话时按钮立即反映新会话的实际状态

---

## 涉及文件

| 文件 | 操作 |
|------|------|
| `deskapp/src/ui/chat-view.ts` | 新增状态字段、修改按钮渲染逻辑、绑定停止事件 |
| `deskapp/src/gateway-client.ts` | 新增对 `chat.stop.response` 消息的监听处理 |
| `deskapp/src/core/types.ts` | 新增 `ChatStopResponsePayload` 接口（若未定义） |

---

## 详细设计

### 1. 状态定义

`ChatView` 中新增两个字段（替代分散的 `streamingMessageEl` 隐式判断）：

```typescript
// 正在进行流式输出的会话集合（支持多会话并发）
private streamingSessions = new Set<string>();

// 正在等待 chat.stop.response 的会话（停止中过渡态）
private stoppingSessions = new Set<string>();
```

> `streamingMessageEl !== null` 仍保留用于当前会话的 DOM 操作，但按钮状态改由 `streamingSessions` 驱动，两者职责分离。

### 2. 状态机流转

```
┌─────────┐   message:send    ┌───────────┐   stopTask()     ┌──────────────┐
│  IDLE   │ ────────────────► │ STREAMING │ ───────────────► │   STOPPING   │
│ 发送按钮 │                   │  停止按钮  │                  │ 停止按钮(禁用) │
└─────────┘                   └───────────┘                  └──────────────┘
     ▲                               │                               │
     │           chat:complete       │        chat.stop.response     │
     └───────────────────────────────┴───────────────────────────────┘
                         chat:error（任一路径）
```

**状态说明**：

| 状态 | 条件 | 按钮表现 |
|------|------|---------|
| IDLE | `!streamingSessions.has(sid)` | 发送图标，可点击 |
| STREAMING | `streamingSessions.has(sid) && !stoppingSessions.has(sid)` | 停止图标，可点击 |
| STOPPING | `stoppingSessions.has(sid)` | 停止图标，禁用（防重复） |

### 3. 状态转换触发点

```typescript
// A. 发送消息 → 进入 STREAMING
// chat-view.ts: sendMessage()
this.streamingSessions.add(this.state.currentSessionId!);
this.updateSendButton();

// B. 流式完成 → 退出 STREAMING / STOPPING
// 监听 'chat:complete' 事件（已有）
this.bus.on('chat:complete', (payload) => {
    if (payload.sessionId === this.state.currentSessionId) {
        this.streamingSessions.delete(payload.sessionId);
        this.stoppingSessions.delete(payload.sessionId);
        this.updateSendButton();
    }
});

// C. 流式错误 → 退出所有状态
// 监听 'chat:error' 事件
this.bus.on('chat:error', (payload) => {
    this.streamingSessions.delete(payload.sessionId);
    this.stoppingSessions.delete(payload.sessionId);
    this.updateSendButton();
});

// D. 停止响应到达 → 退出 STOPPING（STREAMING 由后续 chat:complete 清除）
this.bus.on('chat:stop-response', (payload) => {
    this.stoppingSessions.delete(payload.sessionId);
    this.updateSendButton();
});

// E. 切换会话 → 重新渲染按钮
// 监听 'session:switch' 事件（已有）
this.bus.on('session:switch', () => {
    this.updateSendButton();
});
```

### 4. 停止按钮点击处理

```typescript
private handleStopClick() {
    const sid = this.state.currentSessionId;
    if (!sid) return;
    if (this.stoppingSessions.has(sid)) return; // 防重复

    this.stoppingSessions.add(sid);
    this.updateSendButton(); // 立即禁用按钮

    this.gateway.stopTask(sid);
    // 注意：不等待响应，由 chat:stop-response / chat:complete 事件触发恢复
}
```

### 5. 按钮渲染函数

```typescript
private updateSendButton() {
    const sid = this.state.currentSessionId;
    const isStopping = sid ? this.stoppingSessions.has(sid) : false;
    const isStreaming = sid ? this.streamingSessions.has(sid) : false;

    if (isStreaming) {
        // 切换为停止图标
        this.sendBtn.classList.add('is-streaming');
        this.sendBtn.disabled = isStopping;
        this.sendBtn.setAttribute('aria-label', isStopping ? '停止中...' : '停止生成');
    } else {
        // 恢复发送图标
        this.sendBtn.classList.remove('is-streaming');
        this.sendBtn.disabled = false;
        this.sendBtn.setAttribute('aria-label', '发送');
    }
}
```

### 6. 按钮事件绑定调整

现有代码：
```typescript
this.sendBtn.addEventListener('click', () => this.sendMessage());
```

修改为：
```typescript
this.sendBtn.addEventListener('click', () => {
    const sid = this.state.currentSessionId;
    const isStreaming = sid ? this.streamingSessions.has(sid) : false;
    if (isStreaming) {
        this.handleStopClick();
    } else {
        this.sendMessage();
    }
});
```

> 用同一个按钮 + 单一 `click` 监听器处理两种行为，减少 DOM 操作。

### 7. gateway-client.ts：处理 chat.stop.response

在 `handleMessage` 的消息分发中新增：

```typescript
if (message.type === 'chat.stop.response') {
    const payload = message.payload as { sessionId: string };
    this.bus.emit('chat:stop-response', payload);
}
```

或通过已有的 `progressHandlers` / `messageHandlers` 机制转发，视 `GatewayClient` 的架构选择合适路径（推荐使用 `messageHandlers` 广播）。

### 8. 连接断开容错

监听 `gateway:disconnect` 事件，将所有 streaming / stopping 状态清空：

```typescript
this.bus.on('gateway:disconnect', () => {
    this.streamingSessions.clear();
    this.stoppingSessions.clear();
    this.updateSendButton();
});
```

---

## 测试案例

| 编号 | 场景 | 预期结果 |
|------|------|---------|
| T1 | 用户发送消息 | 按钮立即变为停止图标 |
| T2 | LLM 回复完成 (`chat:complete`) | 按钮恢复发送图标 |
| T3 | 点击停止按钮 | 按钮禁用，`stopTask` 被调用 |
| T4 | 收到 `chat.stop.response` | 按钮恢复可点击停止状态 |
| T5 | 收到 `chat:complete`（停止后） | 按钮恢复发送图标 |
| T6 | 在会话 A 流式输出时切换到会话 B | 按钮反映 B 的状态（若 B 无流式则显示发送） |
| T7 | 快速双击停止按钮 | 只调用一次 `stopTask`（状态防护） |
| T8 | 流式期间 WebSocket 断开 | 按钮恢复发送图标（`gateway:disconnect` 处理） |
| T9 | LLM 回复出错 (`chat:error`) | 按钮恢复发送图标 |
| T10 | 无当前会话时点击发送 | 行为不变（已有逻辑处理） |
