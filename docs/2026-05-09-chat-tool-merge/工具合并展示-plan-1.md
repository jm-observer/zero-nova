# Plan-1: 数据结构与事件处理优化

## 前置依赖

无

## 本次目标（可验证）

1. 确认 `toolUseId` 在 `tool_start` 和 `tool_result` 事件中的传递一致性
2. 在 `ChatView` 类中添加工具结果缓存映射，支持结果到达时快速定位对应的调用卡
3. 修改 `handleToolResult()` 方法，将结果插入到对应的 `tool-use-card` 内部而非作为兄弟元素
4. **修复高严重问题**：确保 streamingMessageEl 未创建时结果也能被缓存

## 涉及文件

- `deskapp/src/ui/chat-view.ts` — 核心修改文件
- `deskapp/src/core/types.ts` — 可能需要扩展 `ProgressEvent` 类型

## 详细设计

### 1.1 工具结果缓存映射

**修复高严重问题 2**：使用复合键 `(sessionId, toolUseId)` 避免跨会话缓存错绑。

在 `ChatView` 类中添加新属性，用于缓存等待中的工具结果：

```typescript
// 工具结果缓存：sessionId -> toolUseId -> 未渲染的结果数据
// 使用 Map<sessionId, Map<toolUseId, {result, isError}>> 结构
private pendingToolResults = new Map<string, Map<string, {
    result: string;
    isError: boolean;
    sessionId: string;  // 冗余字段，便于清理
}>>();

/**
 * 获取指定会话的缓存 Map
 */
private getPendingResultsForSession(sessionId: string): Map<string, {
    result: string;
    isError: boolean;
    sessionId: string;
}> | undefined {
    return this.pendingToolResults.get(sessionId);
}

/**
 * 设置缓存结果
 */
private setPendingResult(sessionId: string, toolUseId: string, data: {
    result: string;
    isError: boolean;
}): void {
    if (!this.pendingToolResults.has(sessionId)) {
        this.pendingToolResults.set(sessionId, new Map());
    }
    this.pendingToolResults.get(sessionId)!.set(toolUseId, { ...data, sessionId });
}

/**
 * 获取缓存结果
 */
private getPendingResult(sessionId: string, toolUseId: string): {
    result: string;
    isError: boolean;
} | undefined {
    const sessionMap = this.pendingToolResults.get(sessionId);
    return sessionMap?.get(toolUseId);
}

/**
 * 删除缓存结果
 */
private deletePendingResult(sessionId: string, toolUseId: string): void {
    const sessionMap = this.pendingToolResults.get(sessionId);
    if (sessionMap) {
        sessionMap.delete(toolUseId);
        // 清理空会话
        if (sessionMap.size === 0) {
            this.pendingToolResults.delete(sessionId);
        }
    }
}

/**
 * 清理指定会话的所有缓存结果
 */
private clearPendingResultsForSession(sessionId: string): void {
    this.pendingToolResults.delete(sessionId);
}
```

**设计理由**：
- 工具结果可能比工具调用稍晚到达（网络延迟、异步执行）
- 缓存机制确保即使结果先于调用到达也不会丢失
- 使用会话维度复合键避免跨会话缓存错绑
- 与现有 `streamingSessions` 模式保持一致

### 1.2 修改 `handleToolStart()` 方法

**当前逻辑**（chat-view.ts:1280-1298）：
- 直接创建 `tool-use-card` 兄弟元素插入到 `markdown-body`

**修改后逻辑**：
- 创建 `tool-use-card` 兄弟元素，但内部预留结果区域
- 检查 `pendingToolResults` 是否有缓存结果
- 如果有缓存结果，立即渲染到内部

```typescript
private handleToolStart(event: any) {
    const { toolName, args, toolUseId, sessionId } = event;
    
    // 使用会话 ID，如果没有则使用当前会话 ID
    const currentSessionId = sessionId || this.state.currentSessionId;
    
    // ... 现有逻辑创建 streamingMessageEl ...
    if (!this.streamingMessageEl) {
        this.streamingMessageEl = this.createStreamingMessage();
        this.messagesContainer.appendChild(this.streamingMessageEl);
    }
    
    const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
    if (markdownBody) {
        // 检查是否有缓存的结果
        const cachedResult = this.getPendingResult(currentSessionId, toolUseId);
        
        const html = `
            <div class="tool-use-card collapsible" data-tool-use-id="${toolUseId}">
                <div class="tool-name">🛠️ ${toolName} <span class="collapse-icon">⌄</span></div>
                <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                <div class="tool-log-streamer hidden"></div>
                <div class="tool-result-container" data-rel-id="${toolUseId}"></div>
            </div>
        `;
        markdownBody.insertAdjacentHTML('beforeend', html);
        
        // 如果有缓存结果，立即渲染
        if (cachedResult) {
            this.renderCachedResult(toolUseId, cachedResult);
            this.deletePendingResult(currentSessionId, toolUseId);
        }
        
        this.scrollToBottom();
    }
}
```

### 1.3 修改 `handleToolResult()` 方法

**修复高严重问题 1**：将"是否可缓存"的判断前置，至少在 streamingMessageEl 为空时也能按 toolUseId 入缓存。

**当前逻辑**（chat-view.ts:1301-1343）：
- 创建 `tool-result-card` 兄弟元素插入到 `markdown-body`
- 使用 `data-rel-id` 匹配 `toolUseId`
- **问题**：`if (!this.streamingMessageEl) return;` 导致 streamingMessageEl 未创建时直接返回

**修改后逻辑**：
- 首先尝试在 `streamingMessageEl` 内部查找对应的 `tool-result-container`
- 如果找到，直接渲染到容器内部
- 如果没找到（结果先到达或 streamingMessageEl 不存在），缓存结果

```typescript
private handleToolResult(event: any) {
    const { toolUseId, result, isError, sessionId } = event;
    this.handleProjectManagerResult(event);
    
    // 使用会话 ID，如果没有则使用当前会话 ID
    const currentSessionId = sessionId || this.state.currentSessionId;
    
    // 修复：先检查是否有缓存，即使 streamingMessageEl 不存在也要缓存
    const hasCachedResult = this.getPendingResult(currentSessionId, toolUseId);
    
    if (!this.streamingMessageEl) {
        // streamingMessageEl 不存在，直接缓存结果
        if (!hasCachedResult) {
            this.setPendingResult(currentSessionId, toolUseId, { result, isError });
        }
        return;
    }

    const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
    if (markdownBody) {
        // 尝试在内部查找结果容器
        const resultContainer = markdownBody.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`);
        
        if (resultContainer) {
            // 渲染到容器内部
            this.renderResultContent(resultContainer, result, isError, toolUseId);
            this.scrollToBottom();
            this.updateMinimap();
            
            // 15s 后自动折叠
            setTimeout(() => {
                const toolCard = resultContainer.closest('.tool-use-card');
                if (toolCard) toolCard.classList.add('collapsed');
                this.updateMinimap();
            }, 15000);
        } else if (!hasCachedResult) {
            // 结果先到达，缓存起来
            this.setPendingResult(currentSessionId, toolUseId, { result, isError });
        }
    }
}
```

### 1.4 新增辅助方法

```typescript
/**
 * 渲染缓存的结果内容到指定容器
 */
private renderResultContent(
    container: HTMLElement,
    result: string,
    isError: boolean,
    toolUseId: string
) {
    const originalContent = result || '';
    let displayContent = '';
    let isErrorCode = this.hasExitCodeError(originalContent, isError);
    
    try {
        const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
        if (parsed && typeof parsed === 'object' && parsed.output_summary) {
            displayContent = renderMarkdown(parsed.output_summary);
        } else {
            displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
        }
    } catch (e) {
        displayContent = escapeHtml(String(originalContent));
    }

    const html = `
        <div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}">
            <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
            <div class="tool-result-content">${displayContent}</div>
        </div>
    `;
    container.innerHTML = html;
}

/**
 * 渲染缓存的结果（当 tool_start 先到达时调用）
 */
private renderCachedResult(toolUseId: string, cached: { result: string; isError: boolean; sessionId: string }) {
    const markdownBody = this.streamingMessageEl?.querySelector('.markdown-body');
    const container = markdownBody?.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`);
    
    if (container) {
        this.renderResultContent(container, cached.result, cached.isError, toolUseId);
    }
}

/**
 * 在会话切换时清理缓存（清理离开前会话）
 */
private onSessionChanged(previousSessionId?: string) {
    if (previousSessionId) {
        this.clearPendingResultsForSession(previousSessionId);
    }
}
```

### 1.5 会话切换清理

在 `init()` 方法中添加会话切换监听：

```typescript
this.bus.on(Events.SESSION_CHANGED, (payload: any) => {
    // ... 现有逻辑 ...
    // 仅清理离开前会话，避免误清理新会话缓存
    const previousSessionId = payload?.fromSessionId || this.lastSessionId;
    this.onSessionChanged(previousSessionId);
    this.lastSessionId = payload?.toSessionId || this.state.currentSessionId;
});
```

## 测试案例

### 正常路径
1. 工具调用先到达 → 工具结果后到达 → 结果渲染到调用卡内部
2. 工具结果先到达 → 工具调用后到达 → 结果渲染到调用卡内部
3. 多个工具并行调用 → 每个结果正确匹配到对应调用卡

### 边界条件
1. 工具结果到达时 `streamingMessageEl` 尚未创建 → 结果被缓存，后续 `tool_start` 时渲染
2. 工具结果到达时 `streamingMessageEl` 已切换会话 → 结果被丢弃（现有行为）
3. 同一工具多次调用（如循环）→ 每次使用新的 `toolUseId` 避免冲突
4. **跨会话**：不同会话的相同 `toolUseId` 不会互相干扰

### 异常场景
1. `toolUseId` 为空字符串 → 缓存和查找都使用空字符串作为 key
2. `result` 为 `null` 或 `undefined` → `renderResultContent` 中的默认值处理
3. `sessionId` 为 `null` → 使用 `this.state.currentSessionId` 作为 fallback
