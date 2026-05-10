# Plan-2: DOM 结构重构与渲染逻辑

## 前置依赖

Plan-1（事件处理优化完成）

## 本次目标（可验证）

1. 重构 `renderMessages` 中的 `buildToolHtml` 函数，使其输出嵌套结构
2. 确保消息恢复（`MESSAGES_UPDATED`）时嵌套结构正确重建
3. 确保 `tool_log` 事件在嵌套结构下仍能正确定位日志区域
4. **修复高严重问题 3**：明确单通道算法，避免重复渲染和顺序错乱

## 涉及文件

- `deskapp/src/ui/chat-view.ts` — `buildToolHtml`、`renderMessages`
- `deskapp/src/styles/main/chat.css` — 可能需要调整样式

## 详细设计

### 2.1 `buildToolHtml` 函数分析

**当前逻辑**（chat-view.ts:1017-1083）：
```typescript
// 遍历 content 中的 blocks
for each block:
    if type === 'tool_use':
        输出 <div class="tool-use-card" data-tool-use-id="xxx">...</div>
    else if type === 'tool_result':
        输出 <div class="tool-result-card" data-rel-id="xxx">...</div>
```

两个卡片是**并列输出**的。

### 2.2 修复高严重问题 3：单通道算法

**问题分析**：
- 当前设计"先收集再按 tool_use 输出"，但实现又"for message.content 全量遍历 + tool_use 分支推入 html"
- 对 interleaving block 的顺序依赖较强
- `processedResultIds` 只记录命中 result，无法避免某些重复场景（如重复 id、无 id）

**解决方案**：采用**三阶段算法**：
1. 第一阶段：遍历 content，为每个 `tool_use` 创建占位，为每个 `tool_result` 建立映射
2. 第二阶段：遍历 content，将 `tool_result` 填充到对应的占位容器
3. 第三阶段：输出未匹配的孤立 `tool_result`

### 2.3 代码实现

```typescript
/**
 * 统一解析 toolUseId，兼容多种字段名
 */
private resolveToolUseId(blockOrEvent: any): string {
    return blockOrEvent.id || blockOrEvent.toolUseId || blockOrEvent.tool_use_id || '';
}

private buildToolHtml(message: Message): string {
    if (!Array.isArray(message.content)) return '';
    
    // 阶段 1：收集 tool_use 和 tool_result，建立映射
    const toolUseMap = new Map<string, any>();  // toolUseId -> tool_use block
    const resultMap = new Map<string, any>();   // toolUseId -> tool_result block
    const toolUseOrder: string[] = [];          // 保持 tool_use 的出现顺序
    
    for (const block of message.content) {
        if (block.type === 'tool_use' || block.type === 'tool_call') {
            const id = this.resolveToolUseId(block);
            if (id) {
                toolUseMap.set(id, block);
                if (!toolUseOrder.includes(id)) {
                    toolUseOrder.push(id);
                }
            }
        } else if (block.type === 'tool_result') {
            const id = this.resolveToolUseId(block);
            if (id) {
                resultMap.set(id, block);
            }
        }
    }
    
    // 阶段 2：按顺序输出 tool_use（包含对应的 tool_result）
    let htmlParts: string[] = [];
    const processedResultIds = new Set<string>();
    
    for (const toolUseId of toolUseOrder) {
        const toolUseBlock = toolUseMap.get(toolUseId);
        const resultBlock = resultMap.get(toolUseId);
        
        const name = toolUseBlock?.name || toolUseBlock?.toolName || '';
        const args = toolUseBlock?.args || toolUseBlock?.input || {};
        
        let resultHtml = '';
        if (resultBlock) {
            processedResultIds.add(toolUseId);
            resultHtml = this.buildToolResultInline(resultBlock);
        }
        
        // 嵌套结构：tool_use 包含 tool_result
        const html = `
            <div class="tool-use-card collapsible collapsed" data-tool-use-id="${toolUseId}">
                <div class="tool-name">🛠️ ${name} <span class="collapse-icon">⌄</span></div>
                <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                <div class="tool-log-streamer hidden"></div>
                <div class="tool-result-container" data-rel-id="${toolUseId}">
                    ${resultHtml}
                </div>
            </div>
        `;
        htmlParts.push(html);
    }
    
    // 阶段 3：输出孤立的 tool_result（没有对应 tool_use）
    for (const block of message.content) {
        if (block.type === 'tool_result') {
            const id = this.resolveToolUseId(block);
            if (id && !processedResultIds.has(id)) {
                const originalContent = block.content || block.result || block.output || '';
                let displayContent = '';
                let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
                
                try {
                    const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
                    if (parsed && typeof parsed === 'object') {
                        if (parsed.output_summary) {
                            displayContent = renderMarkdown(parsed.output_summary);
                        } else {
                            displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                        }
                    } else {
                        displayContent = escapeHtml(String(originalContent));
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
                htmlParts.push(html);
            }
        }
    }
    
    return htmlParts.join('');
}

/**
 * 构建内嵌的工具结果 HTML（用于嵌套结构）
 */
private buildToolResultInline(block: any): string {
    const originalContent = block.content || block.result || block.output || '';
    let displayContent = '';
    let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
    
    try {
        const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
        if (parsed && typeof parsed === 'object') {
            if (parsed.output_summary) {
                displayContent = renderMarkdown(parsed.output_summary);
                
                if (parsed.logs && Array.isArray(parsed.logs) && parsed.logs.length > 0) {
                    displayContent += `
                        <details class="subagent-logs-detail" style="margin-top: 12px; border: 1px solid var(--border-color); border-radius: 6px; overflow: hidden;">
                            <summary style="padding: 8px 12px; background: var(--bg-secondary); cursor: pointer; font-size: 0.85em; font-weight: 500; display: flex; align-items: center; gap: 8px;">
                                <span class="icon">📜</span> ${t('chat.subagent_logs')}
                            </summary>
                            <div style="padding: 0; background: #000; color: #fff; font-family: var(--font-code); font-size: 0.8em; max-height: 300px; overflow-y: auto;">
                                <pre style="margin: 0; padding: 12px; white-space: pre-wrap; line-height: 1.4;">${escapeHtml(parsed.logs.join(''))}</pre>
                            </div>
                        </details>`;
                }

                if (parsed.workspace_files && Array.isArray(parsed.workspace_files) && parsed.workspace_files.length > 0) {
                    displayContent += `<div class="tool-result-files" style="margin-top: 10px; font-size: 0.9em; color: var(--text-secondary);">
                        📁 ${t('chat.files_created', parsed.workspace_files.length)}: ${parsed.workspace_files.join(', ')}
                    </div>`;
                }
            } else {
                displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
            }
        } else {
            displayContent = escapeHtml(String(originalContent));
        }
    } catch (e) {
        displayContent = escapeHtml(String(originalContent));
    }

    return `<div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}">
        <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
        <div class="tool-result-content">${displayContent}</div>
    </div>`;
}
```

### 2.4 `handleToolLog` 兼容性确认

**当前逻辑**（chat-view.ts:1232-1258）：
```typescript
const card = this.messagesContainer.querySelector(`.tool-use-card[data-tool-use-id="${toolUseId}"]`);
const streamer = card.querySelector('.tool-log-streamer');
```

**无需修改**：因为 `tool-use-card` 的位置不变，`.tool-log-streamer` 仍在卡片内部，查询逻辑保持一致。

## 测试案例

### 正常路径
1. 从后端恢复消息 → `buildToolHtml` 正确输出嵌套结构
2. 包含 `output_summary` 的工具结果 → 嵌套渲染 Markdown
3. 包含 `workspace_files` 的工具结果 → 嵌套渲染文件列表

### 边界条件
1. `tool_use` 没有对应 `tool_result` → 渲染空的 `tool-result-container`
2. `tool_result` 没有对应 `tool_use` → 渲染为独立卡片
3. 多个 `tool_use` 对应同一个 `tool_result`（重复 ID）→ 只渲染一次
4. `tool_use` 和 `tool_result` 交错出现 → 保持正确顺序

### 异常场景
1. `block.id` 和 `block.toolUseId` 同时存在 → 优先使用 `block.id`
2. `block.args` 为 `null` → `JSON.stringify(null, null, 2)` 输出 `"null"`
3. `toolUseId` 为空字符串 → 使用空字符串作为 key
