import { beforeEach, describe, expect, it, vi } from 'vitest';

import { EventBus, Events } from '../core/event-bus';
import { AppState } from '../core/state';
import { AgentConsoleView } from '../ui/agent-console-view';

describe('AgentConsoleView prompt reload sync', () => {
    beforeEach(() => {
        document.body.innerHTML = `
            <div id="console-prompt-version"></div>
            <div id="console-prompt-preview"></div>
            <button id="console-prompt-reload-btn"></button>
            <div id="console-memory-hits"></div>
        `;
    });

    it('重载成功且版本立即一致时进入 synced', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('s1');

        const gatewayClient = {
            reloadSessionSystemPrompt: vi.fn().mockResolvedValue({ versionAfter: 'version-new-12345678', changed: true }),
            getSessionRuntime: vi.fn().mockResolvedValue({
                sessionId: 's1',
                systemPromptState: { version: 'version-new-12345678', updatedAt: Date.now() },
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            }),
            getSessionPromptPreview: vi.fn().mockResolvedValue({ systemPrompt: 'new prompt' }),
        };
        state.setGatewayClient(gatewayClient as any);

        const notifications: Array<{ type: string; message: string }> = [];
        bus.on(Events.NOTIFICATION, payload => notifications.push(payload as any));

        const view = new AgentConsoleView(state, bus);
        await (view as any).handlePromptReload('s1');

        expect((view as any).promptReloadState).toBe('synced');
        expect(gatewayClient.getSessionRuntime).toHaveBeenCalledTimes(1);
        expect(notifications.some(item => item.type === 'success')).toBe(true);
    });

    it('重载成功但版本始终不一致时停在 awaiting_sync', async () => {
        vi.useFakeTimers();
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('s1');

        const gatewayClient = {
            reloadSessionSystemPrompt: vi.fn().mockResolvedValue({ versionAfter: 'version-target', changed: true }),
            getSessionRuntime: vi.fn().mockResolvedValue({
                sessionId: 's1',
                systemPromptState: { version: 'version-old', updatedAt: Date.now() },
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            }),
            getSessionPromptPreview: vi.fn().mockResolvedValue({ systemPrompt: 'old prompt' }),
        };
        state.setGatewayClient(gatewayClient as any);

        const view = new AgentConsoleView(state, bus);
        const pending = (view as any).handlePromptReload('s1');
        await vi.runAllTimersAsync();
        await pending;

        expect((view as any).promptReloadState).toBe('awaiting_sync');
        expect(gatewayClient.getSessionRuntime).toHaveBeenCalledTimes(3);
        vi.useRealTimers();
    });

    it('重载失败时进入 failed 且保留旧内容', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('s1');
        state.updateSessionResourceState('s1', 'prompt', state.setLoadedResource({ systemPrompt: 'old prompt' } as any));
        state.updateSessionResourceState(
            's1',
            'runtime',
            state.setLoadedResource({
                sessionId: 's1',
                systemPromptState: { version: 'version-old', updatedAt: Date.now() },
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            } as any),
        );

        const gatewayClient = {
            reloadSessionSystemPrompt: vi.fn().mockRejectedValue(new Error('boom')),
        };
        state.setGatewayClient(gatewayClient as any);

        const notifications: Array<{ type: string; message: string }> = [];
        bus.on(Events.NOTIFICATION, payload => notifications.push(payload as any));

        const view = new AgentConsoleView(state, bus);
        await (view as any).handlePromptReload('s1');

        expect((view as any).promptReloadState).toBe('failed');
        const promptState = state.getSessionResourceState('s1', 'prompt') as any;
        expect(promptState?.data?.systemPrompt).toBe('old prompt');
        expect(notifications.some(item => item.type === 'error')).toBe(true);
    });
});
