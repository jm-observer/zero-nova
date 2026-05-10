# Plan-3: CSS 样式与交互优化

## 前置依赖

Plan-2（DOM 结构重构完成）

## 本次目标（可验证）

1. 添加 `tool-result-container` 的 CSS 样式，确保嵌套结果在视觉上层次分明
2. 优化折叠/展开动画，使嵌套结构下的交互更流畅
3. 确保小地图（Minimap）在嵌套结构下仍能正确计算标记位置
4. 保持现有 `tool-result-card` 和 `tool-use-card` 的样式兼容
5. **修复中严重问题 4**：点击冒泡处理不误伤结果卡内部交互
6. **修复中严重问题 5**：:has() 降级方案闭环

## 涉及文件

- `deskapp/src/styles/main/chat.css` — 主要样式文件
- `deskapp/src/ui/chat-view.ts` — 折叠联动逻辑

## 详细设计

### 3.1 新增 CSS 样式

```css
/* ===== 工具结果容器（嵌套在 tool-use-card 内部） ===== */

.tool-result-container {
    margin-top: 8px;
    padding: 4px 0 0 0;
    border-top: 1px solid var(--border-color, #e5e7eb);
    min-height: 24px; /* 空容器时保持最小高度 */
    transition: opacity 0.2s ease;
}

/* 结果容器有内容时增加上边距 */
.tool-result-container:has(.tool-result-card) {
    padding-top: 8px;
}

/* ===== 嵌套工具结果卡片的样式微调 ===== */

.tool-use-card .tool-result-card {
    margin: 8px 0 0 0;
    border-left: 3px solid var(--border-color, #e5e7eb);
}

/* 错误状态的工具结果卡片 */
.tool-use-card .tool-result-card.error {
    border-left-color: var(--error-color, #ef4444);
}

/* ===== 折叠状态下的工具结果容器 ===== */

.tool-use-card.collapsed .tool-result-container {
    /* 折叠时减少高度，但保持可见 */
    max-height: 0;
    overflow: hidden;
    transition: max-height 0.3s ease, opacity 0.2s ease;
    opacity: 0.5;
}

.tool-use-card:not(.collapsed) .tool-result-container {
    max-height: 500px;
    opacity: 1;
}

/* ===== 工具调用卡片的样式调整 ===== */

.tool-use-card {
    /* 确保卡片有适当的 padding 容纳嵌套内容 */
    padding: 8px 12px;
    margin: 4px 0;
}

.tool-use-card .tool-log-streamer {
    /* 日志流区域在结果容器之前 */
    max-height: 150px;
    overflow-y: auto;
    margin: 4px 0;
}

.tool-use-card .tool-log-streamer.hidden {
    display: none;
}

/* ===== 工具结果卡片内部样式 ===== */

.tool-result-card .tool-result-header {
    cursor: pointer;
    user-select: none;
    padding: 4px 0;
    font-weight: 500;
}

.tool-result-card .tool-result-content {
    padding: 4px 0;
}

.tool-result-card pre.json-result {
    background: var(--bg-secondary, #f9fafb);
    padding: 8px 12px;
    border-radius: 4px;
    overflow-x: auto;
    font-size: 0.85em;
    margin: 4px 0;
}

/* ===== 折叠图标动画 ===== */

.tool-use-card .collapse-icon,
.tool-result-card .collapse-icon {
    display: inline-block;
    transition: transform 0.2s ease;
}

.tool-use-card.collapsed .collapse-icon,
.tool-result-card.collapsed .collapse-icon {
    transform: rotate(-90deg);
}

/* ===== 工具结果文件列表样式 ===== */

.tool-result-files {
    padding: 4px 0;
}

/* ===== 子代理日志详情样式 ===== */

.subagent-logs-detail {
    margin-top: 12px;
    border: 1px solid var(--border-color, #e5e7eb);
    border-radius: 6px;
    overflow: hidden;
}

.subagent-logs-detail summary {
    padding: 8px 12px;
    background: var(--bg-secondary, #f9fafb);
    cursor: pointer;
    font-size: 0.85em;
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 8px;
}

.subagent-logs-detail pre {
    margin: 0;
    padding: 12px;
    white-space: pre-wrap;
    line-height: 1.4;
}

/* ===== :has() 降级方案（修复中严重问题 5） ===== */

/* 当 :has() 不支持时，通过 JS 添加 has-result 类 */
.no-has-support .tool-result-container.has-result {
    padding-top: 8px;
}

.no-has-support .tool-result-container:not(.has-result) {
    padding-top: 4px;
}
```

### 3.2 修复中严重问题 4：点击冒泡处理

**问题分析**：
- 当前方案 `if (target.closest('.tool-result-card')) e.stopPropagation();` 会拦截结果卡内所有点击
- 包括 `<details>/<summary>`、链接、复制按钮等内部交互

**解决方案**：只在"结果卡头部折叠按钮"场景阻断，或基于更精确选择器判断。

```typescript
// 在 chat-view.ts 的 bindEvents() 中修改点击事件处理
this.messagesContainer.addEventListener('click', (e) => {
    const target = e.target as HTMLElement;
    const traceCopyBtn = target.closest('.message-trace-copy-btn') as HTMLButtonElement | null;
    if (traceCopyBtn) {
        void this.handleTraceCopyClick(traceCopyBtn);
        return;
    }
    
    // 修复：允许结果卡内部的 <details>/<summary>、链接、按钮等交互
    // 只阻止对折叠按钮的点击冒泡
    const isResultCardHeader = target.closest('.tool-result-header');
    const isResultCardContentInteractive = target.closest('a, button, details, summary');
    
    if (isResultCardHeader && !isResultCardContentInteractive) {
        // 点击结果卡头部，阻止冒泡到父卡片
        e.stopPropagation();
    }
    
    // 允许点击整个 Header 或 Header 内部的任何元素
    const header = target.closest('.tool-name, .tool-result-header');
    if (header) {
        const card = header.closest('.tool-use-card, .tool-result-card');
        if (card) {
            card.classList.toggle('collapsed');
            // 触发布局更刷新，确保导航条位置正确
            this.updateMinimap();
        }
    } else {
        // 如果直接点击了已折叠卡片的空白处，也执行展开
        const collapsedCard = target.closest('.collapsible.collapsed');
        if (collapsedCard) {
            collapsedCard.classList.remove('collapsed');
            this.updateMinimap();
        }
    }
});
```

### 3.3 :has() 降级方案闭环

**修复中严重问题 5**：补充完整的降级逻辑。

```typescript
// 在 init() 中检测 :has() 支持并添加降级类
init() {
    console.log('[ChatView] Initializing...');
    new OrchestrationView(this.bus, this.messagesContainer, () => this.state.currentSessionId);
    this.ensureProjectMenu();
    this.bindEvents();
    
    // 检测 :has() 支持
    if (!CSS.supports('selector(:has(.foo))')) {
        document.documentElement.classList.add('no-has-support');
    }
    
    // ... 其他初始化逻辑 ...
}

// 在渲染结果时，为容器添加 has-result 类
private renderResultContent(
    container: HTMLElement,
    result: string,
    isError: boolean,
    toolUseId: string
) {
    // ... 现有逻辑 ...
    
    const html = `
        <div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}">
            <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
            <div class="tool-result-content">${displayContent}</div>
        </div>
    `;
    container.innerHTML = html;
    container.classList.add('has-result');  // 添加 has-result 类用于降级
}
```

### 3.4 折叠联动逻辑

**修改方案**：

```typescript
// 在 handleToolResult 中修改 15 秒折叠逻辑
setTimeout(() => {
    // 查找嵌套结构中的 tool-use-card（通过 closest 向上查找）
    const toolCard = markdownBody.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`)
        ?.closest('.tool-use-card');
    
    if (toolCard) {
        toolCard.classList.add('collapsed');
    }
    this.updateMinimap();
}, 15000);
```

### 3.5 小地图兼容性

**当前逻辑**（chat-view.ts:1460-1512）：
- 遍历所有 `.message` 元素
- 计算每个消息在滚动容器中的百分比位置
- 放置标记点

**无需修改**：因为嵌套结构不改变 `.message` 元素的数量和位置，小地图逻辑保持兼容。

### 3.6 视觉层次设计

```
┌─────────────────────────────────────────┐
│  🛠️ readFile <span class="collapse-icon">⌄</span>  │  ← tool-use-card
│  ┌─────────────────────────────────────┐│
│  │  "path": "src/main.ts"              ││  ← tool-args
│  └─────────────────────────────────────┘│
│  ┌─────────────────────────────────────┐│
│  │  [日志流区域 - 可选]                  ││  ← tool-log-streamer
│  └─────────────────────────────────────┘│
│  ─────────────────────────────────────  │
│  ┌─────────────────────────────────────┐│  ← tool-result-container
│  │  🔍 工具结果 <span class="collapse-icon">⌄</span>           ││
│  │  ┌─────────────────────────────────┐││  ← tool-result-card
│  │  │  文件内容预览...                 │││
│  │  └─────────────────────────────────┘││
│  └─────────────────────────────────────┘│
└─────────────────────────────────────────┘
```

## 测试案例

### 正常路径
1. 工具调用展开 → 工具结果可见
2. 工具调用折叠 → 工具结果隐藏
3. 工具结果卡片独立折叠/展开
4. 结果卡内部的 `<details>` 展开/折叠不受影响

### 边界条件
1. 工具结果内容为空 → 容器保持最小高度
2. 工具结果内容很长 → 支持滚动
3. 快速连续折叠/展开 → 动画不卡顿

### 异常场景
1. CSS 变量未定义 → 使用 fallback 值
2. `:has()` 选择器不支持的浏览器 → 降级为 JS 控制
3. 点击结果卡内部的链接 → 不被阻止冒泡
