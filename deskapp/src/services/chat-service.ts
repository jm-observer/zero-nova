import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { GatewayClient } from '../gateway-client';

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

        // Listen for outgoing messages
        this.bus.on('message:send', async (payload: { text: string }) => {
            console.log('[ChatService] Outgoing message:', payload.text);
            
            // Optimistically add user message
            this.state.addMessage({
                id: 'tmp-' + Date.now(),
                role: 'user',
                content: payload.text,
                createdAt: Date.now()
            });

            await this.sendMessage(payload.text);
        });

        this.bus.on(Events.SESSION_CHANGED, async () => {
             // Refresh messages to get persistent IDs and updated state
             if (this.state.currentSessionId) {
                 const messages = await this.client.getMessages(this.state.currentSessionId);
                 this.state.setMessages(messages as any);
             }
        });

        // Handle manual session creation from UI
        this.bus.on(Events.SESSION_CREATE, async (payload: { title?: string }) => {
            const title = payload?.title || 'New Chat';
            const agentId = this.state.currentAgentId || 'default';
            try {
                const session = await this.client.createSession({ title, agentId }); 
                this.state.addSession(session as any);
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
    }

    private async sendMessage(text: string) {
        if (!this.state.currentSessionId) {
             const title = text.length > 20 ? text.substring(0, 20) + '...' : text;
             const agentId = this.state.currentAgentId || 'default';
             const session = await this.client.createSession({ title, agentId });
             this.state.addSession(session as any);
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

    private handleProgress(event: any) {
        if (event.type === 'token') {
            this.bus.emit('token', event.token);
        } else if (event.type === 'complete') {
            this.bus.emit('chat:complete', event);
        } else if (event.type === 'tool_start') {
            this.bus.emit('tool:start', event);
        }
    }
}
