import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml, formatTime } from '../utils/html';

export class ChatView {
    private messagesContainer: HTMLElement;
    private messageInput: HTMLTextAreaElement;
    private sendBtn: HTMLButtonElement;
    private inspectBtn: HTMLButtonElement;
    
    private streamingMessageEl: HTMLElement | null = null;
    private streamingContent = ''; // 仅作向后兼容和备份
    private currentIntentText: string | null = null;
    private layoutObserver: ResizeObserver | null = null;

    constructor(private state: AppState, private bus: EventBus) {
        this.messagesContainer = document.getElementById('messages') as HTMLElement;
        this.messageInput = document.getElementById('message-input') as HTMLTextAreaElement;
        this.sendBtn = document.getElementById('send-btn') as HTMLButtonElement;
        this.inspectBtn = document.getElementById('inspect-btn') as HTMLButtonElement;
    }

    init() {
        console.log('[ChatView] Initializing...');
        this.bindEvents();
        
        // 监听消息容器大小变化，更新右侧的 Minimap 导航条
        if (window.ResizeObserver) {
            this.layoutObserver = new ResizeObserver(() => {
                this.updateMinimap();
            });
            this.layoutObserver.observe(this.messagesContainer);
        }
        
        this.bus.on(Events.SESSION_SELECTED, () => {
            this.updateHeaderTitle();
        });
        
        this.bus.on(Events.SESSION_CHANGED, (payload: any) => {
             console.log('[ChatView] Session changed:', payload.previousSessionId, '->', payload.sessionId);
             // 如果是从初始状态 (null) 切换到第一个会话，说明正在通过首条消息建立会话，
             // 此时应保留当前显示的乐观消息（用户刚发出的那一条），不执行清空。
             if (payload.previousSessionId === null && payload.sessionId !== null) {
                 return;
             }
             this.clear();
        });

        this.bus.on(Events.MESSAGES_UPDATED, (payload: any) => {
             console.log('[ChatView] Messages updated, rendering...', payload.messages.length);
             this.renderMessages(payload.messages);
        });

        this.bus.on(Events.MESSAGE_ADDED, (payload: any) => {
             console.log('[ChatView] New message added:', payload.message.id);
             this.addMessage(payload.message);
        });

        this.bus.on('token', (payload: { sessionId: string, token: string }) => {
             if (payload.sessionId === this.state.currentSessionId) {
                 this.appendToken(payload.token);
             }
        });

        this.bus.on('chat:complete', (payload: any) => {
             if (payload.sessionId === this.state.currentSessionId) {
                 console.log('[ChatView] Chat complete, resetting streaming state');
                 this.streamingMessageEl = null;
                 this.streamingContent = '';
             }
        });

        this.bus.on(Events.CHAT_INTENT, (payload: any) => {
            this.handleIntent(payload);
        });
        
        this.bus.on('tool:log', (event: any) => {
            this.handleToolLog(event);
        });

        this.bus.on('tool:start', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleToolStart(event);
            }
        });

        this.bus.on('tool:result', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleToolResult(event);
            }
        });
        
        this.bus.on('chat:error', (payload: any) => {
            if (payload.sessionId === this.state.currentSessionId) {
                this.handleChatError(payload);
            }
        });

        this.bus.on('system:log', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleSystemLog(event);
            }
        });

        this.bus.on('chat:iteration', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleIteration(event);
            }
        });
    }

    private bindEvents() {
        this.sendBtn.addEventListener('click', () => this.sendMessage());
        this.inspectBtn?.addEventListener('click', () => {
            this.state.setConsoleVisible(!this.state.consoleVisible);
        });
        this.messageInput.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                this.sendMessage();
            }
        });

        this.messagesContainer.addEventListener('contextmenu', (e) => this.handleContextMenu(e));
        document.addEventListener('click', () => this.hideContextMenu());

        // 工具卡片折叠监听
        this.messagesContainer.addEventListener('click', (e) => {
            const target = e.target as HTMLElement;
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
    }

    private hideContextMenu() {
        const menu = document.getElementById('chat-context-menu');
        if (menu) menu.remove();
    }

    private handleContextMenu(e: MouseEvent) {
        const messageEl = (e.target as HTMLElement).closest('.message');
        if (!messageEl) return;

        e.preventDefault();
        this.hideContextMenu();

        const index = parseInt(messageEl.getAttribute('data-index') || '-1');
        if (index === -1) return;

        const menu = document.createElement('div');
        menu.id = 'chat-context-menu';
        menu.className = 'context-menu';
        menu.style.position = 'fixed';
        menu.style.left = `${e.clientX}px`;
        menu.style.top = `${e.clientY}px`;
        menu.style.zIndex = '1000';

        const item = document.createElement('div');
        item.className = 'context-menu-item';
        item.innerHTML = `<span class="icon">content_copy</span> ${t('chat.clone_session')}`;
        item.onclick = () => {
             this.bus.emit(Events.SESSION_COPY, { id: this.state.currentSessionId, index });
             this.hideContextMenu();
        };

        menu.appendChild(item);
        document.body.appendChild(menu);
    }

    private sendMessage() {
        const text = this.messageInput.value.trim();
        if (!text) return;
        
        // 发送新消息前，确保重置流式状态，避免内容追加到旧的气泡中
        this.streamingMessageEl = null;
        this.streamingContent = '';
        
        this.currentIntentText = null;
        this.bus.emit('message:send', { text });
        this.messageInput.value = '';
    }

    clear() {
        this.messagesContainer.innerHTML = '';
        this.streamingMessageEl = null;
        this.streamingContent = '';
        this.updateMinimap();
    }

    renderMessages(messages: any[]) {
        // 保存当前的流式状态，避免在渲染历史消息时冲掉正在产生的回复
        const prevStreamingEl = this.streamingMessageEl;
        const isStreaming = !!prevStreamingEl;

        this.streamingMessageEl = null;
        this.streamingContent = '';
        
        // 过滤掉 system 角色的消息，避免工作区显得杂乱
        // 同时保留原始索引，以便后续操作（如克隆会话）能对应上正确的后端索引
        const displayMessages = messages
            .map((m, i) => ({ ...m, originalIndex: i }))
            .filter(m => m.role !== 'system');
        
        if (displayMessages.length === 0 && !isStreaming) {
            this.showWelcome();
            return;
        }
        
        this.messagesContainer.innerHTML = displayMessages.map((m) => this.renderMessage(m, m.originalIndex)).join('');
        
        // 如果之前正在流式输出，将其重新追加到容器末尾
        if (isStreaming) {
            this.streamingMessageEl = prevStreamingEl;
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }

        this.scrollToBottom();
        // 因为可能有大量 DOM 发生改变，使用 setTimeout 等待渲染结束再抓取位置
        setTimeout(() => this.updateMinimap(), 50);
    }

    private showWelcome() {
        this.messagesContainer.innerHTML = `
            <div class="welcome-message">
                <h3>${t('chat.welcome_title')}</h3>
                <p>${t('chat.welcome_desc')}</p>
            </div>
        `;
    }

    private hasExitCodeError(content: any, backendIsError: boolean = false): boolean {
        if (backendIsError) return true;
        if (!content) return false;
        
        let c = content;
        if (typeof content === 'string') {
            try {
                c = JSON.parse(content);
            } catch(e) {}
        }
        
        if (typeof c === 'object' && c !== null && c.exit_code !== undefined) {
            return c.exit_code !== 0;
        }
        if (typeof content === 'string') {
            if (content.includes('"exit_code":')) {
                return !content.includes('"exit_code": 0') && !content.includes('"exit_code":0');
            } else if (content.includes('exit_code:')) {
                return !content.match(/exit_code:\s*0\b/);
            }
        }
        return false;
    }

    private addMessage(message: any) {
        if (message.role === 'system') return;
        // 使用 state 中的消息总数减 1 作为原始索引
        const index = this.state.messages.length - 1;
        const html = this.renderMessage(message, index);
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
        this.updateMinimap();
    }

    private renderMessage(message: any, index: number): string {
        const isAssistant = message.role === 'assistant';
        const content = message.content;
        const voiceTranscriptState = message.metadata?.voiceTranscriptState as 'pending' | 'final' | undefined;
        let contentHtml = '';
        if (!content) {
            contentHtml = '<span class="empty-content">...</span>';
        } else if (Array.isArray(content)) {
            // 处理 Phase 4 的内容块数组
            contentHtml = content.map((block: any) => {
                const type = block.type;
                if (type === 'text') {
                    const text = typeof block.text === 'string' ? block.text : (block.content || '');
                    return renderMarkdown(text);
                } else if (type === 'thinking') {
                    return `<div class="thinking-block">
                        <div class="thinking-header">${t('chat.thinking')}</div>
                        <div class="thinking-content">${renderMarkdown(block.thinking || '')}</div>
                    </div>`;
                } else if (type === 'tool_use' || type === 'tool_call') {
                    const name = block.name || block.toolName;
                    const input = block.input || block.args;
                    const toolUseId = block.id || block.toolUseId;
                    return `<div class="tool-use-card collapsible collapsed" data-tool-use-id="${toolUseId}">
                        <div class="tool-name">🛠️ ${name} <span class="collapse-icon">⌄</span></div>
                        <pre class="tool-args">${JSON.stringify(input, null, 2)}</pre>
                        <div class="tool-log-streamer hidden"></div>
                    </div>`;
                } else if (type === 'tool_result') {
                    const originalContent = block.content || block.result || block.output || '';
                    let displayContent = '';
                    let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
                    
                    try {
                        const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
                        
                        if (parsed && typeof parsed === 'object') {
                            if (parsed.output_summary) {
                                // Subagent summary rendering
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
                                // Other JSON tools
                                displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                            }
                        } else {
                            displayContent = escapeHtml(String(originalContent));
                        }
                    } catch (e) {
                        displayContent = escapeHtml(String(originalContent));
                    }

                    return `<div class="tool-result-card collapsible collapsed ${isErrorCode ? 'error' : ''}">
                        <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
                        <div class="tool-result-content">${displayContent}</div>
                    </div>`;
                }
                return '';
            }).join('');
        } else {
            // 兼容旧的字符串格式
            const text = typeof content === 'string' ? content : JSON.stringify(content);
            contentHtml = isAssistant ? renderMarkdown(text) : escapeHtml(text);
        }
        
        const isTool = Array.isArray(message.content) && message.content.some((b: any) => b.type === 'tool_result');
        let hasToolError = false;
        if (isTool) {
             hasToolError = message.content.some((b: any) => {
                  if (b.type !== 'tool_result') return false;
                  return this.hasExitCodeError(b.content || b.result || b.output, b.isError);
             });
        }
        const roleClass = isTool ? `tool ${hasToolError ? 'tool-error-msg' : ''}` : message.role;
        
        const intentText = message.metadata?.intentText || (isAssistant && this.currentIntentText ? this.currentIntentText : '');
        const intentHtml = intentText ? `<div class="message-intent">${escapeHtml(intentText)}</div>` : '';
        const voiceBadgeHtml = voiceTranscriptState
            ? `<div class="message-intent">${voiceTranscriptState === 'pending' ? t('voice.recognizing') : t('voice.title')}</div>`
            : '';

        return `
            <div class="message ${roleClass} ${voiceTranscriptState ? `voice-transcript ${voiceTranscriptState}` : ''}" data-index="${index}">
                <div class="message-bubble">
                    ${voiceBadgeHtml}
                    ${intentHtml}
                    <div class="markdown-body">${contentHtml}</div>
                </div>
                <div class="message-time">${formatTime(message.timestamp || message.createdAt)}</div>
            </div>
        `;
    }

    private appendToken(token: string) {
        if (!this.streamingMessageEl) {
            this.streamingMessageEl = this.createStreamingMessage();
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }
        
        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (!markdownBody) return;

        // 查找或创建当前的文本块容器
        // 如果最后一个子元素不是文本块（可能是工具卡片），则新建一个
        let textBlock = markdownBody.lastElementChild as HTMLElement;
        if (!textBlock || !textBlock.classList.contains('streaming-text-block')) {
            textBlock = document.createElement('div');
            textBlock.className = 'streaming-text-block';
            (textBlock as any)._rawContent = ''; // 用于增量 Markdown 渲染
            markdownBody.appendChild(textBlock);
        }

        (textBlock as any)._rawContent += token;
        textBlock.innerHTML = renderMarkdown((textBlock as any)._rawContent);
        
        this.scrollToBottom();
    }

    private createStreamingMessage(): HTMLElement {
        const div = document.createElement('div');
        div.className = 'message assistant streaming';
        
        const intentHtml = this.currentIntentText 
            ? `<div class="message-intent">${escapeHtml(this.currentIntentText)}</div>` 
            : '';

        div.innerHTML = `
            <div class="message-bubble">
                ${intentHtml}
                <div class="markdown-body"></div>
            </div>
        `;
        return div;
    }

    private handleIntent(payload: any) {
        console.log('[ChatView] Handling intent:', payload);
        let text = '';
        switch(payload.intent) {
            case 'chat': text = t('chat.intent_chat'); break;
            case 'resolve': text = t('chat.intent_resolve'); break;
            case 'continue_workflow': text = t('chat.intent_continue_workflow'); break;
            case 'address_agent': 
                const agent = this.state.agentsList.find(a => a.id === payload.agentId);
                const name = agent ? agent.name : (payload.agentId || 'Unknown');
                text = t('chat.intent_address_agent').replace('{0}', name);
                break;
        }
        
        if (text) {
            this.currentIntentText = text;
            // 如果已经正在流式输出，动态更新当前的 header
            if (this.streamingMessageEl) {
                const intentEl = this.streamingMessageEl.querySelector('.message-intent');
                if (intentEl) {
                    intentEl.textContent = text;
                } else {
                    const bubble = this.streamingMessageEl.querySelector('.message-bubble');
                    bubble?.insertAdjacentHTML('afterbegin', `<div class="message-intent">${escapeHtml(text)}</div>`);
                }
            }
        }
    }

    private handleToolLog(event: any) {
        const { toolUseId, log, stream, sessionId } = event;
        // 隔离非当前会话的工具日志
        if (sessionId && sessionId !== this.state.currentSessionId) return;

        // 查找对应的 tool-use-card
        const card = this.messagesContainer.querySelector(`.tool-use-card[data-tool-use-id="${toolUseId}"]`);
        if (!card) return;

        const streamer = card.querySelector('.tool-log-streamer');
        if (!streamer) return;

        streamer.classList.remove('hidden');
        const line = document.createElement('div');
        line.className = `log-line ${stream || 'stdout'}`;
        line.textContent = log;
        streamer.appendChild(line);

        // 自动滚动日志区域
        streamer.scrollTop = streamer.scrollHeight;
        
        // 同时滚动整个消息区域
        this.scrollToBottom();
    }

    private handleChatError(payload: any) {
        let text = '';
        if (payload.type === 'iteration_limit') {
            text = t('chat.error_iteration_limit').replace('{0}', String(payload.iteration || 10));
        } else {
            text = payload.message || t('common.unknown_error');
        }

        const html = `
            <div class="message system error">
                <div class="message-bubble">
                    <div class="error-header">⚠️ ${t('common.error')}</div>
                    <div class="error-content">${escapeHtml(text)}</div>
                </div>
            </div>
        `;
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    private handleToolStart(event: any) {
        const { toolName, args, toolUseId } = event;
        if (!this.streamingMessageEl) {
            this.streamingMessageEl = this.createStreamingMessage();
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }

        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (markdownBody) {
            const html = `
                <div class="tool-use-card collapsible" data-tool-use-id="${toolUseId}">
                    <div class="tool-name">🛠️ ${toolName} <span class="collapse-icon">⌄</span></div>
                    <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                    <div class="tool-log-streamer hidden"></div>
                </div>
            `;
            markdownBody.insertAdjacentHTML('beforeend', html);
            this.scrollToBottom();
        }
    }

    private handleToolResult(event: any) {
        const { toolUseId, result, isError } = event;
        
        // 我们需要找到对应的 tool-use-card 并在其后插入结果，或者直接在 streaming message 中寻找
        if (!this.streamingMessageEl) return;

        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (markdownBody) {
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
                <div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}" data-rel-id="${toolUseId}">
                    <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
                    <div class="tool-result-content">${displayContent}</div>
                </div>
            `;
            markdownBody.insertAdjacentHTML('beforeend', html);
            this.scrollToBottom();
            this.updateMinimap();

            // 5s 后自动折叠工具调用和结果
            setTimeout(() => {
                const toolCard = markdownBody.querySelector(`.tool-use-card[data-tool-use-id="${toolUseId}"]`);
                const resultCard = markdownBody.querySelector(`.tool-result-card[data-rel-id="${toolUseId}"]`);
                if (toolCard) toolCard.classList.add('collapsed');
                if (resultCard) resultCard.classList.add('collapsed');
            }, 5000);
        }
    }

    private handleSystemLog(event: any) {
        const { log } = event;
        // 过滤常见的 Agent 迭代反馈，避免干扰用户视线
        if (log.includes('Agent iteration')) return;

        const isError = log.toLowerCase().includes('failed') || log.toLowerCase().includes('error');
        const roleClass = isError ? 'system log error' : 'system log';

        const html = `
            <div class="message ${roleClass}">
                <div class="message-bubble">
                    <div class="markdown-body">${escapeHtml(log)}</div>
                </div>
            </div>
        `;
        
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    /**
     * 处理 AI 迭代进度，更新状态栏
     */
    private handleIteration(event: any) {
        const { iteration } = event;
        // 构建友好的状态文本
        const statusText = `Agent Running (${iteration}/30)`;
        // 发布全局状态更新，TitleBar 会捕获并更新顶部的红绿灯/文字
        this.bus.emit(Events.GATEWAY_STATUS, { status: 'running', text: statusText });
        
        // 可选：如果 5s 内没有任何新进度，Titlebar 会保持这个状态直到任务完成（chat:complete 回调会重置状态）
    }

    private scrollToBottom() {
        this.messagesContainer.scrollTop = this.messagesContainer.scrollHeight;
    }

    private updateMinimap() {
        const minimap = document.getElementById('chat-minimap');
        const minimapMarkers = document.getElementById('chat-minimap-markers');
        if (!minimap || !minimapMarkers) return;

        const messagesContainer = this.messagesContainer;
        const scrollHeight = messagesContainer.scrollHeight;
        // 如果内容不需要滚动，可以隐藏或仍保留。按比例的话，不超出一满屏会显得比较稀疏。
        // if (scrollHeight <= clientHeight && messagesContainer.children.length < 2) {
        //    minimap.style.display = 'none';
        //    return;
        // } else {
        //    minimap.style.display = 'block';
        // }

        let hasAnyError = false;
        minimapMarkers.innerHTML = '';

        const messages = messagesContainer.querySelectorAll('.message');
        if (messages.length === 0) return;

        messages.forEach((msg) => {
            const htmlMsg = msg as HTMLElement;
            const isUser = htmlMsg.classList.contains('user');
            const isToolError = htmlMsg.classList.contains('tool-error-msg');
            
            if (isToolError) hasAnyError = true;

            if (isUser || isToolError) {
                // 计算相对位置 (百分比)
                // 以消息元素的中间位置为准，由于 scrollHeight 包含所有的 content，
                // 计算比例能大致映射到 minimap 的竖线上。
                const offsetTop = htmlMsg.offsetTop;
                // 添加一个小偏移，使得中心点更准
                const topPercent = ((offsetTop + (htmlMsg.clientHeight / 2)) / scrollHeight) * 100;
                
                const marker = document.createElement('div');
                marker.className = `chat-minimap-marker ${isUser ? 'user' : 'tool-error'}`;
                // 限制最高值为 98% 避免掉出底端
                marker.style.top = `${Math.min(98, Math.max(2, topPercent))}%`;
                marker.title = isUser ? 'User Message' : 'Tool Error (exit_code != 0)';
                
                // 用户点击跳转
                marker.addEventListener('click', () => {
                    htmlMsg.scrollIntoView({ behavior: 'smooth', block: 'center' });
                    // 如果有高亮需求可以在这里补充
                    htmlMsg.style.transition = 'background-color 0.5s ease';
                    const origBg = htmlMsg.style.backgroundColor;
                    // Flash effect
                    htmlMsg.style.backgroundColor = 'rgba(99, 102, 241, 0.15)';
                    setTimeout(() => { htmlMsg.style.backgroundColor = origBg; }, 1000);
                });
                
                minimapMarkers.appendChild(marker);
            }
        });

        // 根据是否有 tool error ，让外面的主线变成红色
        if (hasAnyError) {
            minimap.classList.add('has-error');
        } else {
            minimap.classList.remove('has-error');
        }
    }

    private updateHeaderTitle() {
        const titleEl = document.getElementById('chat-header-title');
        if (titleEl) {
            const session = this.state.sessions.find(s => s.id === this.state.currentSessionId);
            const agent = this.state.agentsList.find(a => a.id === this.state.currentAgentId);
            titleEl.textContent = session?.title || agent?.name || 'Chat';
        }
    }
}
