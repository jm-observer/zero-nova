import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
    invoke: invokeMock,
}));

import { EventBus } from '../core/event-bus';
import { AppState } from '../core/state';
import { ChatView } from '../ui/chat-view';

function flushAsync(): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, 0));
}

describe('ChatView @ project picker', () => {
    beforeEach(() => {
        document.body.innerHTML = `
            <div id="messages"></div>
            <div class="input-container">
                <textarea id="message-input"></textarea>
                <button id="send-btn">send</button>
                <button id="inspect-btn">inspect</button>
            </div>
        `;
        invokeMock.mockReset();
        (globalThis as any).ResizeObserver = class {
            observe() {}
            disconnect() {}
        };
    });

    it('输入 @ 后展示目录项并可实时过滤', async () => {
        invokeMock.mockImplementation(async (_cmd: string, args: any) => {
            const relativePath = args?.relativePath ?? '';
            if (!relativePath) {
                return [
                    { name: 'src', relativePath: 'src', isDir: true },
                    { name: 'README.md', relativePath: 'README.md', isDir: false },
                ];
            }
            return [];
        });

        const view = new ChatView(new AppState(new EventBus()), new EventBus());
        view.init();

        const input = document.getElementById('message-input') as HTMLTextAreaElement;
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        const picker = document.querySelector('.project-picker') as HTMLElement;
        expect(picker).not.toBeNull();
        expect(picker.classList.contains('visible')).toBe(true);
        expect(picker.textContent).toContain('src');
        expect(picker.textContent).toContain('README.md');

        input.value = '@read';
        input.setSelectionRange(5, 5);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        expect(picker.textContent).toContain('README.md');
        expect(picker.textContent).not.toContain('src');
    });

    it('键盘选择文件后插入 @相对路径 并补空格', async () => {
        invokeMock.mockResolvedValue([
            { name: 'a.txt', relativePath: 'a.txt', isDir: false },
            { name: 'b.txt', relativePath: 'b.txt', isDir: false },
        ]);

        const view = new ChatView(new AppState(new EventBus()), new EventBus());
        view.init();

        const input = document.getElementById('message-input') as HTMLTextAreaElement;
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
        input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
        await flushAsync();

        expect(input.value).toBe('@b.txt ');
        const picker = document.querySelector('.project-picker') as HTMLElement;
        expect(picker.classList.contains('visible')).toBe(false);
    });
});
