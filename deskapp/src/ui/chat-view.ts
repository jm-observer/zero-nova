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
        this.bus.emit('message:send', { text });
        this.messageInput.value = '';
    }

    clear() {
        this.messagesContainer.innerHTML = '';
    }

    renderMessages(messages: any[]) {
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

    private scrollToBottom() {
        this.messagesContainer.scrollTop = this.messagesContainer.scrollHeight;
    }
}
