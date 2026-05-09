import { beforeEach, describe, expect, it, vi } from 'vitest';

import { EventBus } from '../core/event-bus';
import { AppState } from '../core/state';
import { t } from '../i18n';
import { ChatView } from '../ui/chat-view';

function flushAsync(): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, 0));
}

describe('ChatView http trace copy', () => {
    beforeEach(() => {
        document.body.innerHTML = `
            <div id="messages"></div>
            <div class="input-container">
                <textarea id="message-input"></textarea>
                <button id="send-btn">send</button>
                <button id="inspect-btn">inspect</button>
            </div>
        `;
        (globalThis as any).ResizeObserver = class {
            observe() {}
            disconnect() {}
        };
        (globalThis.navigator as any).clipboard = {
            writeText: vi.fn().mockResolvedValue(undefined),
        };
    });

    it('assistant 消息有完整 trace 时可复制 pretty JSON', async () => {
        const bus = new EventBus();
        const toastSpy = vi.fn();
        bus.on('toast', toastSpy);
        const state = new AppState(bus);
        const view = new ChatView(state, bus);
        view.init();

        const message = {
            id: 'msg-assistant-1',
            role: 'assistant',
            content: 'ok',
            createdAt: Date.now(),
            metadata: {
                providerHttpTrace: {
                    requestBody: { model: 'gpt', x: 1 },
                    responseBody: { id: 'resp-1' },
                    boundMessageId: 'msg-assistant-1',
                },
            },
        };
        state.messages = [message as any];
        view.renderMessages(state.messages as any[]);

        const requestBtn = document.querySelector(
            '.message-trace-copy-btn[data-body-type="request"]',
        ) as HTMLButtonElement | null;
        const responseBtn = document.querySelector(
            '.message-trace-copy-btn[data-body-type="response"]',
        ) as HTMLButtonElement | null;
        expect(requestBtn?.disabled).toBe(false);
        expect(responseBtn?.disabled).toBe(false);

        requestBtn?.click();
        await flushAsync();
        expect(navigator.clipboard.writeText).toHaveBeenCalledWith(JSON.stringify({ model: 'gpt', x: 1 }, null, 2));
        expect(toastSpy).toHaveBeenCalledWith({ message: t('chat.copy_request_body_success') });
    });

    it('缺失 responseBody 时响应复制按钮置灰', () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        const view = new ChatView(state, bus);
        view.init();

        state.messages = [
            {
                id: 'msg-assistant-2',
                role: 'assistant',
                content: 'ok',
                createdAt: Date.now(),
                metadata: {
                    providerHttpTrace: {
                        requestBody: { model: 'gpt' },
                        boundMessageId: 'msg-assistant-2',
                    },
                },
            } as any,
        ];
        view.renderMessages(state.messages as any[]);

        const responseBtn = document.querySelector(
            '.message-trace-copy-btn[data-body-type="response"]',
        ) as HTMLButtonElement | null;
        expect(responseBtn?.disabled).toBe(true);
    });

    it('boundMessageId 不匹配时禁止复制并提示异常', async () => {
        const bus = new EventBus();
        const toastSpy = vi.fn();
        bus.on('toast', toastSpy);
        const state = new AppState(bus);
        const view = new ChatView(state, bus);
        view.init();

        state.messages = [
            {
                id: 'msg-assistant-3',
                role: 'assistant',
                content: 'ok',
                createdAt: Date.now(),
                metadata: {
                    providerHttpTrace: {
                        requestBody: { model: 'gpt' },
                        responseBody: { id: 'resp' },
                        boundMessageId: 'another-id',
                    },
                },
            } as any,
        ];
        view.renderMessages(state.messages as any[]);

        const requestBtn = document.querySelector(
            '.message-trace-copy-btn[data-body-type="request"]',
        ) as HTMLButtonElement | null;
        expect(requestBtn?.disabled).toBe(true);
        requestBtn?.click();
        await flushAsync();

        expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
        expect(toastSpy).not.toHaveBeenCalledWith({ message: t('chat.copy_request_body_success') });
    });

    it('剪贴板失败时提示复制失败', async () => {
        (navigator.clipboard.writeText as any).mockRejectedValueOnce(new Error('denied'));

        const bus = new EventBus();
        const toastSpy = vi.fn();
        bus.on('toast', toastSpy);
        const state = new AppState(bus);
        const view = new ChatView(state, bus);
        view.init();

        state.messages = [
            {
                id: 'msg-assistant-4',
                role: 'assistant',
                content: 'ok',
                createdAt: Date.now(),
                metadata: {
                    providerHttpTrace: {
                        requestBody: { model: 'gpt' },
                        responseBody: { id: 'resp' },
                        boundMessageId: 'msg-assistant-4',
                    },
                },
            } as any,
        ];
        view.renderMessages(state.messages as any[]);

        const requestBtn = document.querySelector(
            '.message-trace-copy-btn[data-body-type="request"]',
        ) as HTMLButtonElement | null;
        requestBtn?.click();
        await flushAsync();

        expect(toastSpy).toHaveBeenCalledWith({ message: t('chat.copy_body_failed') });
    });
});

