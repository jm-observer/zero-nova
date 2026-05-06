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

    it('单个 provider error 不再导致全局 error 态，除非所有 provider 都失效', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        
        // 一个 provider 报错，另一个正常
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [
                {
                    provider: 'openai',
                    scope: 'orchestration',
                    status: 'auth_failed',
                    checkedAt: Date.now(),
                    message: 'Invalid Key'
                },
                {
                    provider: 'anthropic',
                    scope: 'execution',
                    status: 'healthy',
                    checkedAt: Date.now(),
                }
            ],
        });

        const dot = document.querySelector('#status-indicator .dot') as HTMLElement;
        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        const indicator = document.querySelector('#status-indicator') as HTMLElement;

        // 应该是 Ready (Green) 而不是 Error (Red)
        expect(dot.className).toBe('dot ready');
        expect(text.textContent).toBe('status.gateway_connected_provider_healthy');
        // 应该包含诊断信息
        expect(indicator.title).toContain('orchestration: auth_failed (Invalid Key)');
    });

    it('所有 provider 都失效时显示 error 态', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [
                {
                    provider: 'openai',
                    scope: 'orchestration',
                    status: 'auth_failed',
                    checkedAt: Date.now(),
                }
            ],
        });

        const dot = document.querySelector('#status-indicator .dot') as HTMLElement;
        const text = document.querySelector('#status-indicator .text') as HTMLElement;

        expect(dot.className).toBe('dot error');
        expect(text.textContent).toBe('status.gateway_connected_provider_error');
    });

    it('runtime status text 优先级高于 provider 状态（当网关已连接时）', () => {
        const bus = new EventBus();
        const view = new TitleBarView({} as any, bus);
        view.init();

        bus.emit(Events.GATEWAY_STATUS, { connectionStatus: 'connected' });
        bus.emit(Events.PROVIDER_HEALTH_UPDATED, {
            providers: [{ provider: 'openai', scope: 'orchestration', status: 'healthy', checkedAt: Date.now() }],
        });
        bus.emit(Events.RUNTIME_STATUS_TEXT, { text: 'Agent Running (2/30)' });

        const text = document.querySelector('#status-indicator .text') as HTMLElement;
        const dot = document.querySelector('#status-indicator .dot') as HTMLElement;
        
        expect(text.textContent).toBe('Agent Running (2/30)');
        expect(dot.className).toBe('dot running');
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
