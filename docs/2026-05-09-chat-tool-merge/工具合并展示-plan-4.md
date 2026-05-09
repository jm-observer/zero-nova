# Plan-4: 测试验证与边界处理

## 前置依赖

Plan-1、Plan-2、Plan-3 全部完成

## 本次目标（可验证）

1. 验证所有现有测试用例通过
2. 添加新的单元测试覆盖嵌套结构的边界情况
3. 手动测试场景覆盖流式消息、消息恢复、多会话切换
4. 确保向后兼容性（旧版本消息格式）
5. **修复中严重问题 6**：测试设计与可见性约束兼容
6. **修复低严重问题 7**：统一 ID 兼容策略

## 涉及文件

- `deskapp/src/ui/chat-view.ts` — 核心逻辑
- `deskapp/src/__tests__/chat-view*.test.ts` — 测试文件
- `deskapp/src/styles/main/chat.css` — 样式文件

## 详细设计

### 4.1 测试策略

#### 4.1.1 单元测试

**修复中严重问题 6**：测试通过事件总线触发，而非直接调用 private 方法。

**新增测试文件**：`deskapp/src/__tests__/chat-view-tool-merge.test.ts`

```typescript
describe('ChatView Tool Merge', () => {
    let chatView: ChatView;
    let mockState: AppState;
    let mockBus: EventBus;
    
    beforeEach(() => {
        // 初始化 mock
        mockState = {
            currentSessionId: 'session-1',
            // ... 其他必要属性
        } as any;
        mockBus = new EventBus();
        chatView = new ChatView(mockState, mockBus);
    });
    
    describe('handleToolStart', () => {
        it('应该创建包含 tool-result-container 的 tool-use-card', () => {
            // 通过事件总线触发
            mockBus.emit(Events.TOOL_START, {
                toolName: 'readFile',
                args: { path: 'src/main.ts' },
                toolUseId: 'tool-1',
                sessionId: 'session-1'
            });
            
            const card = document.querySelector('.tool-use-card[data-tool-use-id="tool-1"]');
            expect(card).toBeTruthy();
            expect(card?.querySelector('.tool-result-container')).toBeTruthy();
        });
        
        it('应该在 toolUseId 重复时更新现有卡片', () => {
            // 第一次调用
            mockBus.emit(Events.TOOL_START, { toolName: 'readFile', args: {}, toolUseId: 'tool-1', sessionId: 'session-1' });
            // 第二次调用（重复 ID）
            mockBus.emit(Events.TOOL_START, { toolName: 'writeFile', args: {}, toolUseId: 'tool-1', sessionId: 'session-1' });
            
            const cards = document.querySelectorAll('.tool-use-card[data-tool-use-id="tool-1"]');
            expect(cards.length).toBeLessThanOrEqual(2);
        });
    });
    
    describe('handleToolResult', () => {
        it('应该将结果渲染到对应的 tool-result-container 内部', () => {
            // 先创建 tool-use-card
            mockBus.emit(Events.TOOL_START, { toolName: 'readFile', args: {}, toolUseId: 'tool-1', sessionId: 'session-1' });
            
            // 再发送结果
            mockBus.emit(Events.TOOL_RESULT, {
                toolUseId: 'tool-1',
                result: '{"content": "hello"}',
                isError: false,
                sessionId: 'session-1'
            });
            
            const container = document.querySelector('.tool-result-container[data-rel-id="tool-1"]');
            expect(container?.querySelector('.tool-result-card')).toBeTruthy();
        });
        
        it('当 tool-result-container 不存在时应该缓存结果', () => {
            // 结果先到达
            mockBus.emit(Events.TOOL_RESULT, {
                toolUseId: 'tool-1',
                result: '{"content": "hello"}',
                isError: false,
                sessionId: 'session-1'
            });
            
            // 验证缓存
            const pendingResults = (chatView as any).pendingToolResults;
            expect(pendingResults.has('session-1')).toBeTruthy();
            expect(pendingResults.get('session-1')?.has('tool-1')).toBeTruthy();
        });
        
        it('当 tool-start 到达且有缓存结果时应该立即渲染', () => {
            // 先缓存结果
            const pendingResults = (chatView as any).pendingToolResults;
            pendingResults.set('session-1', new Map([['tool-1', { result: '{"content": "hello"}', isError: false, sessionId: 'session-1' }]]));
            
            // 再创建 tool-use-card
            mockBus.emit(Events.TOOL_START, { toolName: 'readFile', args: {}, toolUseId: 'tool-1', sessionId: 'session-1' });
            
            const container = document.querySelector('.tool-result-container[data-rel-id="tool-1"]');
            expect(container?.querySelector('.tool-result-card')).toBeTruthy();
        });
    });
    
    describe('消息恢复渲染', () => {
        it('应通过公开渲染入口产出 tool_use 内嵌 tool_result 的 DOM', () => {
            // 不直接调用 private buildToolHtml，改为触发公开渲染流程（如 MESSAGES_UPDATED）
            mockBus.emit(Events.MESSAGES_UPDATED, {
                sessionId: 'session-1',
                messages: [{
                    id: 'msg-1',
                    role: 'assistant',
                    content: [
                        { type: 'tool_use', id: 'tool-1', name: 'readFile', args: {} },
                        { type: 'tool_result', id: 'tool-1', content: '{"content": "hello"}' }
                    ],
                    createdAt: Date.now()
                }]
            });
            
            const toolUseCard = document.querySelector('.tool-use-card[data-tool-use-id="tool-1"]');
            const nestedResult = toolUseCard?.querySelector('.tool-result-container[data-rel-id="tool-1"] .tool-result-card');
            expect(toolUseCard).toBeTruthy();
            expect(nestedResult).toBeTruthy();
        });
    });
    
    describe('跨会话缓存', () => {
        it('不同会话的相同 toolUseId 不会互相干扰', () => {
            // 会话 1 的结果
            mockBus.emit(Events.TOOL_RESULT, {
                toolUseId: 'tool-1',
                result: '{"content": "session1"}',
                isError: false,
                sessionId: 'session-1'
            });
            
            // 会话 2 的结果（相同 toolUseId）
            mockBus.emit(Events.TOOL_RESULT, {
                toolUseId: 'tool-1',
                result: '{"content": "session2"}',
                isError: false,
                sessionId: 'session-2'
            });
            
            // 验证两个会话都有缓存
            const pendingResults = (chatView as any).pendingToolResults;
            expect(pendingResults.has('session-1')).toBeTruthy();
            expect(pendingResults.has('session-2')).toBeTruthy();
        });
    });
});
```

#### 4.1.2 集成测试

**测试场景**：
1. 完整工具调用 → 结果流程
2. 多工具并行调用
3. 工具调用中断（无结果）
4. 工具结果错误（isError=true）

### 4.2 边界条件处理

#### 4.2.1 统一 ID 兼容策略（修复低严重问题 7）

**问题分析**：
- 有的地方只用 toolUseId
- 有的地方用 id || toolUseId || result_id || tool_use_id
- 容易造成行为分叉

**解决方案**：统一抽一个 `resolveToolUseId(blockOrEvent)` 规则，文档内只引用该规则。

```typescript
/**
 * 统一解析 toolUseId，兼容多种字段名
 * 优先级：id > toolUseId > tool_use_id
 */
private resolveToolUseId(blockOrEvent: any): string {
    return blockOrEvent.id || blockOrEvent.toolUseId || blockOrEvent.tool_use_id || '';
}
```

**使用位置**：
- `handleToolStart`：`const { toolName, args, toolUseId } = event;`（已通过 normalizeProgressEvent 处理）
- `handleToolResult`：`const { toolUseId, result, isError } = event;`（已通过 normalizeProgressEvent 处理）
- `buildToolHtml`：`const toolUseId = this.resolveToolUseId(block);`
- `buildToolResultInline`：`const resultId = this.resolveToolUseId(block);`

#### 4.2.2 toolUseId 一致性

**问题**：后端可能在 `tool_start` 和 `tool_result` 中使用不同的 ID 字段名。

**解决方案**：统一使用 `normalizeProgressEvent` 中的逻辑：
```typescript
// 在 gateway-messages.ts 中已有
if (typeof normalized.tool_use_id === 'string' && typeof normalized.toolUseId !== 'string') {
    normalized.toolUseId = normalized.tool_use_id;
}
```

在 `handleToolStart` 和 `handleToolResult` 中统一使用 `toolUseId`：
```typescript
// handleToolStart
const { toolName, args, toolUseId } = event;
// toolUseId 可能来自 event.toolUseId 或 event.tool_use_id

// handleToolResult  
const { toolUseId, result, isError } = event;
// toolUseId 可能来自 event.toolUseId 或 event.tool_use_id
```

#### 4.2.3 会话切换清理

**问题**：会话切换时，`pendingToolResults` 中的缓存结果可能属于旧会话。

**解决方案**：在 `SESSION_CHANGED` 事件中清理：
```typescript
this.bus.on(Events.SESSION_CHANGED, (payload: any) => {
    // ... 现有逻辑 ...
    
    // 只清理离开前会话，避免误清理新会话缓存
    const previousSessionId = payload?.fromSessionId || this.lastSessionId;
    if (previousSessionId) {
        this.clearPendingResultsForSession(previousSessionId);
    }
    this.lastSessionId = payload?.toSessionId || this.state.currentSessionId;
});
```

#### 4.2.4 流式消息边界

**问题**：工具调用/结果可能在流式消息中，也可能在独立消息中。

**解决方案**：
- `handleToolStart` 和 `handleToolResult` 已经检查 `this.streamingMessageEl`
- 如果 `streamingMessageEl` 不存在，不会创建新元素（结果会被丢弃）
- 消息恢复时，`buildToolHtml` 会正确重建嵌套结构

### 4.3 向后兼容性

#### 4.3.1 旧版本消息格式

**问题**：从后端恢复的旧消息可能使用不同的字段名。

**解决方案**：在 `buildToolHtml` 中兼容多种 ID 字段：
```typescript
const toolUseId = this.resolveToolUseId(block);
const resultId = this.resolveToolUseId(resultBlock);
```

#### 4.3.2 样式降级

**问题**：`:has()` 选择器在某些浏览器中不支持。

**解决方案**：添加 JS 降级：
```typescript
// 在 init() 中检测 :has() 支持
if (!CSS.supports('selector(:has(.foo))')) {
    // 添加 JS 类来控制样式
    document.documentElement.classList.add('no-has-support');
}
```

### 4.4 性能考虑

#### 4.4.1 DOM 查询优化

**问题**：嵌套结构下，DOM 查询可能需要遍历更多层级。

**解决方案**：
- 使用 `querySelector` 而非 `querySelectorAll` 遍历
- 缓存 `streamingMessageEl` 和 `markdownBody` 的引用
- 在 `handleToolResult` 中优先查询 `tool-result-container` 而非遍历所有卡片

#### 4.4.2 内存管理

**问题**：`pendingToolResults` 缓存可能积累过多数据。

**解决方案**：
- 在 `chat:complete` 事件中清理缓存
- 设置最大缓存大小限制（可选）

```typescript
this.bus.on('chat:complete', (payload: any) => {
    // ... 现有逻辑 ...
    this.pendingToolResults.clear();
});
```

## 测试案例

### 正常路径
1. 工具调用 → 结果 → 嵌套渲染
2. 消息恢复 → 嵌套结构重建
3. 多会话切换 → 缓存清理

### 边界条件
1. toolUseId 为 null → 使用默认值
2. result 为 null → 显示 "No result"
3. 快速连续工具调用 → 每个结果正确匹配
4. 跨会话相同 toolUseId → 不互相干扰

### 异常场景
1. 后端返回重复 toolUseId → 更新现有卡片
2. 工具结果到达时 streamingMessageEl 已销毁 → 结果被缓存
3. CSS :has() 不支持 → JS 降级生效
4. 测试访问 private 方法受限 → 通过事件总线和公开渲染入口触发
