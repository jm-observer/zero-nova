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
             console.log('[ChatView] Session changed, clearing and rendering...');
             this.clear();
             this.renderMessages(this.state.messages);
        });

        this.bus.on(Events.MESSAGE_ADDED, (payload: any) => {
             console.log('[ChatView] New message added:', payload.message.id);
             this.addMessage(payload.message);
        });

        this.bus.on('token', (token: string) => {
             this.appendToken(token);
        });

        this.bus.on('chat:complete', () => {
             console.log('[ChatView] Chat complete, resetting streaming state');
             this.streamingMessageEl = null;
             this.streamingContent = '';
        });

        this.bus.on(Events.CHAT_INTENT, (payload: any) => {
            this.handleIntent(payload);
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
    }

    private sendMessage() {
        const text = this.messageInput.value.trim();
        if (!text) return;
        
        // 发送新消息前，确保重置流式状态，避免内容追加到旧的气泡中
        this.streamingMessageEl = null;
        this.streamingContent = '';
        
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
        this.messagesContainer.innerHTML = messages.map(m => this.renderMessage(m)).join('');
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
        const html = this.renderMessage(message);
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    private renderMessage(message: any): string {
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
                    return `<div class="tool-use-card">
                        <div class="tool-name">🛠️ ${name}</div>
                        <pre class="tool-args">${JSON.stringify(input, null, 2)}</pre>
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
        
        return `
            <div class="message ${message.role}">
                <div class="message-bubble">
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
        div.innerHTML = '<div class="message-bubble"><div class="markdown-body"></div></div>';
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
            this.showStatusTip(text);
        }
    }

    private showStatusTip(text: string) {
        // 如果已经有 streaming message，说明已经开始生成了，就不再显示引导性的意图了
        if (this.streamingMessageEl) return;

        const html = `
            <div class="message assistant intent-tip">
                <div class="message-bubble">
                    <div class="intent-text">${escapeHtml(text)}</div>
                </div>
            </div>
        `;
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();

        // 5秒后自动淡出（如果还没被开始生成的 token 覆盖/跟随）
        const tips = this.messagesContainer.querySelectorAll('.intent-tip');
        const lastTip = tips[tips.length - 1] as HTMLElement;
        setTimeout(() => {
            if (lastTip && lastTip.parentElement) {
                lastTip.classList.add('fade-out');
                setTimeout(() => lastTip.remove(), 500);
            }
        }, 5000);
    }

    private scrollToBottom() {
        this.messagesContainer.scrollTop = this.messagesContainer.scrollHeight;
    }
}
