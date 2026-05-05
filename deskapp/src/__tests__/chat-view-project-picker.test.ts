import { beforeEach, describe, expect, it, vi } from 'vitest';

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
        (globalThis as any).ResizeObserver = class {
            observe() {}
            disconnect() {}
        };
    });

    it('输入 @ 后请求 session.file_tree.list 并本地过滤', async () => {
        const listSessionFileTree = vi.fn().mockResolvedValue([
            { name: 'src', relativePath: 'src', isDir: true },
            { name: 'README.md', relativePath: 'README.md', isDir: false },
        ]);

        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState('session-1', 'runtime', state.setLoadedResource({ sessionId: 'session-1', totalUsage: { inputTokens: 0, outputTokens: 0 } }));
        state.setGatewayClient({ listSessionFileTree } as any);

        const view = new ChatView(state, bus);
        view.init();

        const input = document.getElementById('message-input') as HTMLTextAreaElement;
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        expect(listSessionFileTree).toHaveBeenCalledWith('session-1', undefined);
        const picker = document.querySelector('.project-picker') as HTMLElement;
        expect(picker.textContent).toContain('src');
        expect(picker.textContent).toContain('README.md');

        input.value = '@read';
        input.setSelectionRange(5, 5);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        expect(listSessionFileTree).toHaveBeenCalledTimes(1);
        expect(picker.textContent).toContain('README.md');
        expect(picker.textContent).not.toContain('src');
    });

    it('下钻目录时带 relative_path，选择后插入 @相对路径', async () => {
        const listSessionFileTree = vi
            .fn()
            .mockResolvedValueOnce([{ name: 'src', relativePath: 'src', isDir: true }])
            .mockResolvedValueOnce([{ name: 'a.txt', relativePath: 'src/a.txt', isDir: false }]);

        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState('session-1', 'runtime', state.setLoadedResource({ sessionId: 'session-1', totalUsage: { inputTokens: 0, outputTokens: 0 } }));
        state.setGatewayClient({ listSessionFileTree } as any);

        const view = new ChatView(state, bus);
        view.init();

        const input = document.getElementById('message-input') as HTMLTextAreaElement;
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        const firstItem = document.querySelector('.project-picker-item.dir') as HTMLButtonElement;
        firstItem.click();
        await flushAsync();

        expect(listSessionFileTree).toHaveBeenNthCalledWith(2, 'session-1', 'src');

        input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
        await flushAsync();
        expect(input.value).toBe('@src/a.txt ');
    });

    it('切换会话后不复用旧会话缓存', async () => {
        const listSessionFileTree = vi.fn().mockResolvedValue([{ name: 'README.md', relativePath: 'README.md', isDir: false }]);
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setGatewayClient({ listSessionFileTree } as any);
        state.updateSessionResourceState('s1', 'runtime', state.setLoadedResource({ sessionId: 's1', totalUsage: { inputTokens: 0, outputTokens: 0 } }));
        state.updateSessionResourceState('s2', 'runtime', state.setLoadedResource({ sessionId: 's2', totalUsage: { inputTokens: 0, outputTokens: 0 } }));

        const view = new ChatView(state, bus);
        view.init();
        const input = document.getElementById('message-input') as HTMLTextAreaElement;

        state.setCurrentSession('s1');
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        state.setCurrentSession('s2');
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        expect(listSessionFileTree).toHaveBeenNthCalledWith(1, 's1', undefined);
        expect(listSessionFileTree).toHaveBeenNthCalledWith(2, 's2', undefined);
    });

    it('runtime project_dir 更新后缓存失效并重新拉取', async () => {
        const onSessionRuntimeUpdated = vi.fn();
        const listSessionFileTree = vi.fn().mockResolvedValue([{ name: 'README.md', relativePath: 'README.md', isDir: false }]);
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState('session-1', 'runtime', state.setLoadedResource({ sessionId: 'session-1', totalUsage: { inputTokens: 0, outputTokens: 0 } }));
        state.setGatewayClient({
            listSessionFileTree,
            onSessionRuntimeUpdated: (cb: (payload: Record<string, unknown>) => void) => {
                onSessionRuntimeUpdated.mockImplementation(cb);
                return () => {};
            },
        } as any);

        const view = new ChatView(state, bus);
        view.init();
        const input = document.getElementById('message-input') as HTMLTextAreaElement;

        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        input.dispatchEvent(new Event('input'));
        await flushAsync();
        expect(listSessionFileTree).toHaveBeenCalledTimes(1);

        onSessionRuntimeUpdated({ sessionId: 'session-1', project_dir: '/new/project' });
        input.dispatchEvent(new Event('input'));
        await flushAsync();
        expect(listSessionFileTree).toHaveBeenCalledTimes(2);
    });

    it('未设置项目目录时显示错误态且不回退本地目录', async () => {
        const listSessionFileTree = vi.fn();
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.setGatewayClient({ listSessionFileTree } as any);

        const view = new ChatView(state, bus);
        view.init();

        const input = document.getElementById('message-input') as HTMLTextAreaElement;
        input.value = '@';
        input.setSelectionRange(1, 1);
        input.dispatchEvent(new Event('input'));
        await flushAsync();

        const picker = document.querySelector('.project-picker') as HTMLElement;
        expect(listSessionFileTree).not.toHaveBeenCalled();
        expect(picker.textContent).toContain('当前会话未设置项目目录');
    });
});
