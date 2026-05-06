import { describe, expect, it, vi } from 'vitest';

import { EventBus } from '../core/event-bus';
import { AppState } from '../core/state';
import { ChatService } from '../services/chat-service';

describe('ChatService', () => {
    it('forwards orchestration args without rewriting field names', () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        const listeners: { progress?: (event: any) => void } = {};
        const client = {
            onProgress: vi.fn((handler: (event: any) => void) => {
                listeners.progress = handler;
            }),
            onChatIntent: vi.fn(),
            addMessageHandler: vi.fn(),
        } as any;
        const service = new ChatService(state, bus, client);
        const received: any[] = [];

        bus.on('orchestration:agent_complete', (payload) => {
            received.push(payload);
        });

        service.init();
        listeners.progress?.({
            type: 'sub_agent_complete',
            sessionId: 'session-1',
            args: {
                planId: 'plan-1',
                stageId: 'stage-1',
                agentId: 'agent-1',
                status: 'failed',
                outputSummary: '',
                error: 'boom',
            },
        });

        expect(received).toEqual([
            {
                sessionId: 'session-1',
                planId: 'plan-1',
                stageId: 'stage-1',
                agentId: 'agent-1',
                status: 'failed',
                outputSummary: '',
                error: 'boom',
            },
        ]);
    });
});
