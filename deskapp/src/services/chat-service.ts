import { AppState } from '../core/state';
import { EventBus } from '../core/event-bus';
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
            this.bus.emit('chat:intent', payload);
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

        this.bus.on('chat:complete', async () => {
             // Refresh messages to get persistent IDs and updated state
             if (this.state.currentSessionId) {
                 const messages = await this.client.getMessages(this.state.currentSessionId);
                 this.state.setMessages(messages as any);
             }
        });
    }

    private async sendMessage(text: string) {
        if (!this.state.currentSessionId) {
             const session = await this.client.createSession('New Chat');
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
