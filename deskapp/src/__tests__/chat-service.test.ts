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

    it('ignores complete events for non-current sessions before fetching messages', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-2');

        const listeners: { progress?: (event: any) => void } = {};
        const client = {
            onProgress: vi.fn((handler: (event: any) => void) => {
                listeners.progress = handler;
            }),
            onChatIntent: vi.fn(),
            addMessageHandler: vi.fn(),
            getMessages: vi.fn(),
        } as any;

        const service = new ChatService(state, bus, client);
        const setMessagesSpy = vi.spyOn(state, 'setMessages');

        service.init();
        await listeners.progress?.({
            type: 'complete',
            sessionId: 'session-1',
            usage: { inputTokens: 1, outputTokens: 2 },
        });

        expect(client.getMessages).not.toHaveBeenCalled();
        expect(setMessagesSpy).not.toHaveBeenCalled();
    });

    it('ignores stale complete responses if session changes while fetching messages', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');

        let resolveMessages: ((messages: any[]) => void) | undefined;
        const listeners: { progress?: (event: any) => Promise<void> } = {};
        const client = {
            onProgress: vi.fn((handler: (event: any) => Promise<void>) => {
                listeners.progress = handler;
            }),
            onChatIntent: vi.fn(),
            addMessageHandler: vi.fn(),
            getMessages: vi.fn(() => new Promise((resolve) => {
                resolveMessages = resolve;
            })),
        } as any;

        const service = new ChatService(state, bus, client);
        const setMessagesSpy = vi.spyOn(state, 'setMessages');

        service.init();
        const pending = listeners.progress?.({
            type: 'complete',
            sessionId: 'session-1',
            usage: { inputTokens: 1, outputTokens: 2 },
        });

        state.setCurrentSession('session-2');
        resolveMessages?.([
            { id: 'assistant-1', role: 'assistant', content: 'done', createdAt: Date.now() },
        ]);
        await pending;

        expect(client.getMessages).toHaveBeenCalledWith('session-1');
        expect(setMessagesSpy).not.toHaveBeenCalled();
    });
});

