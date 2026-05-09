import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { GatewayClient } from '../gateway-client';
import type { MessageTokenUsage, Session } from '../core/types';

export class ChatService {
    constructor(private state: AppState, private bus: EventBus, private client: GatewayClient) {}

    init() {
        console.log('[ChatService] Initializing handlers...');
        this.client.onProgress((event) => {
            console.log('[ChatService] Received progress event:', event.type);
            this.handleProgress(event);
        });
        
        this.client.onChatIntent((payload) => {
            console.log('[ChatService] Received intent:', payload.intent);
            this.bus.emit(Events.CHAT_INTENT, payload);
        });

        // 转发停止响应事件到 EventBus，供 ChatView 退出 STOPPING 状态
        this.client.addMessageHandler((msg) => {
            if (msg.type === 'chat.stop.response' && msg.payload) {
                const payload = msg.payload as { sessionId: string };
                this.bus.emit('chat:stop-response', payload);
            }
        });

        // Listen for outgoing messages
        this.bus.on('message:send', async (payload: { text: string; skipOptimisticMessage?: boolean }) => {
            console.log('[ChatService] Outgoing message:', payload.text);
            
            if (!payload.skipOptimisticMessage) {
                this.state.addMessage({
                    id: 'tmp-' + Date.now(),
                    role: 'user',
                    content: payload.text,
                    createdAt: Date.now()
                });
            }

            await this.sendMessage(payload.text);
        });

        // Handle manual session creation from UI
        this.bus.on(Events.SESSION_CREATE, async (payload: { title?: string }) => {
            // 立即清空当前工作区，提升响应感
            this.state.setCurrentSession(null);
            
            const title = payload?.title || 'New Chat';
            const agentId = this.state.currentAgentId || 'default';
            try {
                const session = await this.client.createSession({ title, agentId }); 
                this.state.addSession(session as Session);
                this.state.setCurrentSession(session.id);
            } catch (err) {
                this.bus.emit('toast', { message: 'Failed to create session: ' + err });
            }
        });

        // Handle session deletion
        this.bus.on(Events.SESSION_DELETE, async (payload: { id: string }) => {
            try {
                await this.client.deleteSession(payload.id);
                this.state.deleteSession(payload.id);
            } catch (err) {
                this.bus.emit('toast', { message: 'Failed to delete session: ' + err });
            }
        });

        this.bus.on(Events.SESSION_COPY, async (payload: { id: string, index?: number }) => {
            try {
                const session = await this.client.copySession(payload.id, payload.index);
                this.state.addSession(session as Session);
                this.state.setCurrentSession(session.id);
            } catch (err) {
                this.bus.emit('toast', { message: 'Failed to clone session: ' + err });
            }
        });
    }

    private async sendMessage(text: string) {
        if (!this.state.currentSessionId) {
             const title = text.length > 20 ? text.substring(0, 20) + '...' : text;
             const agentId = this.state.currentAgentId || 'default';
             const session = await this.client.createSession({ title, agentId });
             this.state.addSession(session as Session);
             this.state.setCurrentSession(session.id);
        }
        
        try {
             await this.client.chat(
                 text,
                 this.state.currentSessionId!
             );
        } catch (err) {
             this.bus.emit('toast', { message: 'Failed to send message: ' + err });
        }
    }

    private async handleProgress(event: any) {
        if (event.type === 'token') {
            this.bus.emit('token', { sessionId: event.sessionId, token: event.token });
        } else if (event.type === 'complete') {
            this.bus.emit('chat:complete', event);
            // Refresh messages after completion to sync persistent state
            if (event.sessionId) {
                const sessionId = event.sessionId;
                if (this.state.currentSessionId !== sessionId) {
                    return;
                }
                const messages = await this.client.getMessages(sessionId);
                // 请求返回后再次确认当前会话，避免异步竞态覆盖新会话内容
                if (this.state.currentSessionId !== sessionId) {
                    return;
                }
                const usage = this.normalizeMessageTokenUsage(event.usage);
                const nextMessages = usage ? this.attachUsageToLastAssistantMessage(messages as any[], usage) : messages;
                this.state.setMessages(nextMessages as any[]);
            }
        } else if (event.type === 'tool_start') {
            this.bus.emit('tool:start', event);
        } else if (event.type === 'tool_result') {
            this.bus.emit('tool:result', event);
        } else if (event.type === 'tool_log') {
            this.bus.emit('tool:log', event);
        } else if (event.type === 'iteration_limit') {
            this.bus.emit('chat:error', {
                type: 'iteration_limit',
                sessionId: event.sessionId,
                iteration: event.iteration
            });
        } else if (event.type === 'iteration') {
            this.bus.emit('chat:iteration', event);
        } else if (event.type === 'system_log') {
            this.bus.emit('system:log', event);
        } else if (event.type === 'orchestration_plan') {
            this.bus.emit('orchestration:plan', { sessionId: event.sessionId, ...(event.args || {}) });
        } else if (event.type === 'sub_agent_spawn') {
            this.bus.emit('orchestration:agent_spawn', { sessionId: event.sessionId, ...(event.args || {}) });
        } else if (event.type === 'sub_agent_log') {
            const args = event.args || {};
            this.bus.emit('orchestration:agent_log', {
                sessionId: event.sessionId,
                agentId: args.agentId,
                log: event.log,
                ...args,
            });
        } else if (event.type === 'sub_agent_complete') {
            this.bus.emit('orchestration:agent_complete', { sessionId: event.sessionId, ...(event.args || {}) });
        } else if (event.type === 'stage_complete') {
            this.bus.emit('orchestration:stage_complete', { sessionId: event.sessionId, ...(event.args || {}) });
        } else if (event.type === 'orchestration_review_start') {
            this.bus.emit('orchestration:review_start', { sessionId: event.sessionId, ...(event.args || {}) });
        } else if (event.type === 'orchestration_complete') {
            this.bus.emit('orchestration:complete', { sessionId: event.sessionId, ...(event.args || {}) });
        }
    }

    private normalizeMessageTokenUsage(usage: unknown): MessageTokenUsage | undefined {
        if (!usage || typeof usage !== 'object') {
            return undefined;
        }
        const record = usage as Record<string, unknown>;
        const inputTokens = Number(record.inputTokens ?? 0);
        const outputTokens = Number(record.outputTokens ?? 0);
        return {
            inputTokens,
            outputTokens,
            totalTokens: inputTokens + outputTokens,
            cacheCreationInputTokens: typeof record.cacheCreationInputTokens === 'number' ? record.cacheCreationInputTokens : undefined,
            cacheReadInputTokens: typeof record.cacheReadInputTokens === 'number' ? record.cacheReadInputTokens : undefined,
        };
    }

    private attachUsageToLastAssistantMessage(messages: any[], usage: MessageTokenUsage): any[] {
        if (!Array.isArray(messages) || messages.length === 0) {
            return messages;
        }

        const nextMessages = [...messages];
        for (let index = nextMessages.length - 1; index >= 0; index -= 1) {
            const message = nextMessages[index];
            if (message?.role !== 'assistant') {
                continue;
            }
            nextMessages[index] = { ...message, tokenUsage: usage };
            break;
        }
        return nextMessages;
    }
}

