# 聊天窗口 Session 切换导致聊天记录丢失问题修复

## 时间

文档创建日期：2026-05-08

## 项目现状

前端聊天窗口在以下场景中存在聊天记录丢失的问题：

1. **场景描述：** 当 AI 正在执行任务（如工具调用）时，聊天窗口会持续渲染 AI 的动作（流式输出 + 工具卡片）
2. **问题触发：** 此时如果用户切换 Session 再切回原 Session，之前的聊天记录会全部丢失
3. **影响范围：** 所有使用前端聊天窗口的用户，特别是在 AI 长时间执行工具调用时使用多 Session 的用户

### 问题复现步骤

1. 在 Session A 中发送一条消息
2. 等待 AI 开始流式输出并执行工具调用（此时聊天窗口显示流式消息 + 工具卡片）
3. 快速切换到 Session B（此时 Session A 的流式输出仍在进行中）
4. 立即切回 Session A
5. **观察结果：** 之前的聊天记录（包括流式消息和工具卡片）全部丢失，可能只显示部分消息或空状态

## 整体目标

修复 Session 切换导致的聊天记录丢失问题，确保在 AI 执行任务期间切换 Session 再切回时，聊天记录能够正确显示。

## 根因分析

### 数据流追踪

**关键组件：**
- `ChatService` - 处理来自 Gateway 的进度事件
- `AppState` - 全局状态管理，维护当前会话和消息列表
- `ChatView` - 前端 UI 渲染，负责消息的 DOM 渲染

**问题数据流：**

```
GatewayClient (chat:complete 事件)
  → ChatService.handleProgress() 
    → state.setMessages(messages) 
      → EventBus 发出 MESSAGES_UPDATED
        → ChatView.renderMessages()  ← 完全清空并重建 DOM
```

### 关键代码路径

**ChatService (chat-service.ts:104-114):**
```typescript
} else if (event.type === 'complete') {
    this.bus.emit('chat:complete', event);
    // Refresh messages after completion to sync persistent state
    if (event.sessionId) {
        const messages = await this.client.getMessages(event.sessionId);
        const usage = this.normalizeMessageTokenUsage(event.usage);
        const nextMessages = usage ? this.attachUsageToLastAssistantMessage(messages as any[], usage) : messages;
        if (event.sessionId === this.state.currentSessionId) {
            this.state.setMessages(nextMessages as any[]);
        }
    }
}
```

**ChatView (chat-view.ts:99-102):**
```typescript
this.bus.on(Events.MESSAGES_UPDATED, (payload: any) => {
    console.log('[ChatView] Messages updated, rendering...', payload.messages.length);
    this.renderMessages(payload.messages);  // ← 完全重绘 DOM
});
```

**ChatView.renderMessages (chat-view.ts:868-898):**
```typescript
renderMessages(messages: any[]) {
    const prevStreamingEl = this.streamingMessageEl;
    const isStreaming = !!prevStreamingEl;

    this.streamingMessageEl = null;
    this.streamingContent = '';
    
    // 完全清空 DOM
    this.messagesContainer.innerHTML = displayMessages.map(...).join('');
    
    // 如果之前正在流式输出，重新追加
    if (isStreaming) {
        this.streamingMessageEl = prevStreamingEl;
        this.messagesContainer.appendChild(this.streamingMessageEl);
    }
}
```

### 问题复现场景

| 步骤 | 操作 | 状态 |
|------|------|------|
| 1 | 用户在 Session A 发送消息 | Session A 开始流式输出 |
| 2 | Session A 正在执行工具调用 | 流式渲染中，`streamingMessageEl` 存在 |
| 3 | 用户快速切换到 Session B | `setCurrentSession` 清空 `messages`，`ChatView.clear()` 清空 DOM |
| 4 | Session A 的 `chat:complete` 事件到达 | `ChatService` 拉取 Session A 的完整消息 |
| 5 | `ChatService.setMessages()` 被调用 | 触发 `MESSAGES_UPDATED` |
| 6 | `ChatView.renderMessages()` 执行 | **DOM 被 Session A 的消息覆盖** |
| 7 | 用户切回 Session A | 看到的是被覆盖后的状态，可能丢失部分中间状态 |

### 核心问题

**`ChatView.renderMessages()` 会完全清空并重建 DOM**，当 `chat:complete` 事件在非当前会话期间到达时，会导致：

1. DOM 被清空并重建为另一个会话的消息
2. 流式消息元素（`streamingMessageEl`）被保存后重新追加，但可能位置不对
3. 如果用户恰好在这时切回原会话，看到的是被覆盖后的状态

## 修复方案

### 方案：在 `ChatService.handleProgress` 中增加会话隔离

**修改文件：** `deskapp/src/services/chat-service.ts`

**修改位置：** `handleProgress` 方法的 `complete` 分支

**修改内容：** 在拉取消息前增加会话隔离检查，如果当前会话已切换，跳过消息刷新，避免 DOM 被意外重建

```typescript
} else if (event.type === 'complete') {
    this.bus.emit('chat:complete', event);
    // Refresh messages after completion to sync persistent state
    if (event.sessionId) {
        // 如果当前会话已切换，跳过消息刷新，避免 DOM 被意外重建
        if (this.state.currentSessionId !== event.sessionId) {
            return;
        }
        const messages = await this.client.getMessages(event.sessionId);
        const usage = this.normalizeMessageTokenUsage(event.usage);
        const nextMessages = usage ? this.attachUsageToLastAssistantMessage(messages as any[], usage) : messages;
        if (event.sessionId === this.state.currentSessionId) {
            this.state.setMessages(nextMessages as any[]);
        }
    }
}
```

### 修复效果

- **修复前：** 当 `chat:complete` 事件到达时，无论当前显示哪个 Session，都会拉取该 Session 的消息并触发 `MESSAGES_UPDATED`，导致 `ChatView.renderMessages()` 清空 DOM 并重建
- **修复后：** 如果 `chat:complete` 事件对应的 Session 不是当前显示的 Session，直接返回，不触发消息刷新，避免 DOM 被意外重建

## 测试案例

### 正常路径测试

| 测试用例 | 预期结果 |
|---------|---------|
| 1. 在 Session A 发送消息，等待 AI 完成，不切换 Session | 聊天记录正常显示，无丢失 |
| 2. 在 Session A 发送消息，快速切换到 Session B，立即切回 Session A | 聊天记录正常显示，无丢失 |
| 3. 在 Session A 发送消息，等待 AI 执行工具调用，切换 Session B，等待 AI 完成，切回 Session A | 聊天记录正常显示，无丢失 |
| 4. 在 Session A 发送消息，等待 AI 完成，切换 Session B，再切回 Session A | 聊天记录正常显示，无丢失 |

### 边界条件测试

| 测试用例 | 预期结果 |
|---------|---------|
| 1. 快速连续切换 Session A → B → A → B → A | 每次切换后聊天记录正确显示 |
| 2. 在 Session A 发送消息后，在 `getMessages` 异步请求期间切换 Session | 不会触发 DOM 重建 |
| 3. 在 Session A 发送消息后，在 `chat:complete` 事件到达前切换 Session | 不会触发 DOM 重建 |

### 异常场景测试

| 测试用例 | 预期结果 |
|---------|---------|
| 1. 网络延迟导致 `chat:complete` 事件延迟到达 | 不会触发 DOM 重建 |
| 2. 在 Session A 发送消息后，Gateway 连接断开并重新连接 | 不会触发 DOM 重建 |

## 风险与待定项

### 已知风险

1. **消息同步延迟：** 修复后，如果用户在 Session A 发送消息后切换到 Session B，Session A 的 `chat:complete` 事件不会立即触发消息刷新。当用户切回 Session A 时，可能会看到旧的消息状态，直到下一次 `chat:complete` 事件或手动刷新
2. **流式消息状态：** 修复后，流式消息状态（`streamingMessageEl`）的保存和恢复逻辑需要确保在 Session 切换时不会丢失

### 待定项

1. **是否需要增加手动刷新按钮：** 如果用户发现消息状态不正确，可能需要手动刷新
2. **是否需要增加消息同步机制：** 在 Session 切换时，可能需要主动拉取该 Session 的最新消息

## 涉及文件

| 文件 | 修改内容 |
|------|---------|
| `deskapp/src/services/chat-service.ts` | 在 `handleProgress` 方法的 `complete` 分支中增加会话隔离检查 |

## 执行顺序

1. 修改 `chat-service.ts` 文件
2. 运行前端开发服务器验证修复效果
3. 执行测试案例
4. 确认所有测试用例通过
