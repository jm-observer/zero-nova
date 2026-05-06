import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
    getCurrentWindow: vi.fn(() => ({ startDragging: vi.fn() })),
}));

import { EventBus, Events } from '../core/event-bus';
import { TitleBarView } from '../ui/titlebar';

describe('TitleBarView status priority', () => {
    beforeEach(() => {
        Object.defineProperty(globalThis, 'localStorage', {
            value: {
                getItem: vi.fn(() => null),
                setItem: vi.fn(),
            },
            configurable: true,
        });

        document.body.innerHTML = `
            <div id="status-indicator">
                <span class="dot"></span>
                <span class="text"></span>
            </div>
            <button id="btn-minimize"></button>
            <button id="btn-maximize"></button>
            <button id="btn-close"></button>
            <button id="theme-toggle">
                <svg class="theme-icon-sun"></svg>
                <svg class="theme-icon-moon"></svg>
            </button>
        `;
    });

    it('provider error 优先级高于 runtime status text', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        bus.emit(Events.RUNTIME_STATUS_TEXT, { text: 'Agent Running (2/30)' });
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [
                {
                    provider: 'openai',
                    scope: 'orchestration',
                    status: 'auth_failed',
                    checkedAt: Date.now(),
                },
            ],
        });

        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        expect(text.textContent).toBe('status.gateway_connected_provider_error');
    });

    it('runtime status text 被 clear 后回退到 provider 状态', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [
                {
                    provider: 'openai',
                    scope: 'orchestration',
                    status: 'healthy',
                    checkedAt: Date.now(),
                },
            ],
        });
        bus.emit(Events.RUNTIME_STATUS_TEXT, { text: 'Agent Running (7/30)' });
        bus.emit(Events.RUNTIME_STATUS_TEXT_CLEAR);

        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        expect(text.textContent).toBe('status.gateway_connected_provider_healthy');
    });

    it('reconnecting/disconnected 时连接态优先，不被 provider 或 runtime 覆盖', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [{ provider: 'openai', scope: 'orchestration', status: 'healthy', checkedAt: Date.now() }],
        });
        bus.emit(Events.RUNTIME_STATUS_TEXT, { text: 'Agent Running (9/30)' });
        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'reconnecting' });

        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        expect(text.textContent).toBe('status.reconnecting');

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'disconnected' });
        expect(text.textContent).toBe('status.disconnected');
    });

    it('恢复 connected 后 provider 聚合态继续生效', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [{ provider: 'openai', scope: 'orchestration', status: 'healthy', checkedAt: Date.now() }],
        });
        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'reconnecting' });
        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });

        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        expect(text.textContent).toBe('status.gateway_connected_provider_healthy');
    });
});
