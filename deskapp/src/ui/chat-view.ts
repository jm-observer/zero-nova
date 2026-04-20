import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml, formatTime } from '../utils/html';

export class ChatView {
    private messagesContainer: HTMLElement;
    private messageInput: HTMLTextAreaElement;
    private sendBtn: HTMLButtonElement;
    private stopBtn: HTMLButtonElement;
    
    private streamingMessageEl: HTMLElement | null = null;
    private streamingContent = '';
    private currentIntentText: string | null = null;

    constructor(private state: AppState, private bus: EventBus) {
        this.messagesContainer = document.getElementById('messages') as HTMLElement;
        this.messageInput = document.getElementById('message-input') as HTMLTextAreaElement;
        this.sendBtn = document.getElementById('send-btn') as HTMLButtonElement;
        this.stopBtn = document.getElementById('stop-btn') as HTMLButtonElement;
    }

    init() {
        console.log('[ChatView] Initializing...');
        this.bindEvents();
        
        this.bus.on(Events.SESSION_CHANGED, () => {
             console.log('[ChatView] Session changed, clearing...');
             this.clear();
             // Don't render yet as state.messages might be stale
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
    }

    private bindEvents() {
        this.sendBtn.addEventListener('click', () => this.sendMessage());
        this.messageInput.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                this.sendMessage();
            }
        });

        this.messagesContainer.addEventListener('contextmenu', (e) => this.handleContextMenu(e));
        document.addEventListener('click', () => this.hideContextMenu());
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
    }

    renderMessages(messages: any[]) {
        this.streamingMessageEl = null;
        this.streamingContent = '';
        
        if (messages.length === 0) {
            this.showWelcome();
            return;
        }
        this.messagesContainer.innerHTML = messages.map((m, i) => this.renderMessage(m, i)).join('');
        this.scrollToBottom();
    }

    private showWelcome() {
        this.messagesContainer.innerHTML = `
            <div class="welcome-message">
                <h3>${t('chat.welcome_title')}</h3>
                <p>${t('chat.welcome_desc')}</p>
            </div>
        `;
    }

    private addMessage(message: any) {
        const index = this.messagesContainer.querySelectorAll('.message').length;
        const html = this.renderMessage(message, index);
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    private renderMessage(message: any, index: number): string {
        const isAssistant = message.role === 'assistant';
        const content = message.content;
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
                    return `<div class="tool-use-card" data-tool-use-id="${toolUseId}">
                        <div class="tool-name">🛠️ ${name}</div>
                        <pre class="tool-args">${JSON.stringify(input, null, 2)}</pre>
                        <div class="tool-log-streamer hidden"></div>
                    </div>`;
                } else if (type === 'tool_result') {
                    return `<div class="tool-result-card ${block.isError ? 'error' : ''}">
                        <div class="tool-result-header">${t('chat.tool_result')}</div>
                        <div class="tool-result-content">${escapeHtml(String(block.content || block.result || ''))}</div>
                    </div>`;
                }
                return '';
            }).join('');
        } else {
            // 兼容旧的字符串格式
            const text = typeof content === 'string' ? content : JSON.stringify(content);
            contentHtml = isAssistant ? renderMarkdown(text) : escapeHtml(text);
        }
        
        const intentText = message.metadata?.intentText || (isAssistant && this.currentIntentText ? this.currentIntentText : '');
        const intentHtml = intentText ? `<div class="message-intent">${escapeHtml(intentText)}</div>` : '';

        return `
            <div class="message ${message.role}" data-index="${index}">
                <div class="message-bubble">
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
        this.streamingContent += token;
        const contentEl = this.streamingMessageEl.querySelector('.markdown-body');
        if (contentEl) {
            contentEl.innerHTML = renderMarkdown(this.streamingContent);
        }
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

    private scrollToBottom() {
        this.messagesContainer.scrollTop = this.messagesContainer.scrollHeight;
    }
}
