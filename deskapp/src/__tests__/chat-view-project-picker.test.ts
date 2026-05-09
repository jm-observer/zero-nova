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
                <div class="input-row">
                    <textarea id="message-input"></textarea>
                    <button id="send-btn">send</button>
                    <button id="inspect-btn">inspect</button>
                </div>
            </div>
        `;
        (globalThis as any).ResizeObserver = class {
            observe() {}
            disconnect() {}
        };
    });

    it('requests and filters session file tree after at mention', async () => {
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

    it('navigates relative_path directory and inserts selected path', async () => {
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

    it('does not reuse old session cache after session switch', async () => {
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

    it('invalidates cache after runtime project_dir update', async () => {
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


    it('updates project button label after runtime project_dir update', async () => {
        const onSessionRuntimeUpdated = vi.fn();
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState(
            'session-1',
            'runtime',
            state.setLoadedResource({
                sessionId: 'session-1',
                projectDir: '/old/project',
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            } as any)
        );
        state.setGatewayClient({
            onSessionRuntimeUpdated: (cb: (payload: Record<string, unknown>) => void) => {
                onSessionRuntimeUpdated.mockImplementation(cb);
                return () => {};
            },
        } as any);

        const view = new ChatView(state, bus);
        view.init();

        const trigger = document.querySelector('.project-menu-trigger') as HTMLButtonElement;
        expect(trigger.textContent).toContain('chat.project_not_set');

        onSessionRuntimeUpdated({
            sessionId: 'session-1',
            projectDir: '/new/project',
            totalUsage: { inputTokens: 0, outputTokens: 0 },
        });

        expect(trigger.textContent).toContain('project');
        expect(state.getSessionResourceState('session-1', 'runtime')?.data?.projectDir).toBe('/new/project');
    });

    it('refreshes project button label after ProjectManager success', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState(
            'session-1',
            'runtime',
            state.setLoadedResource({
                sessionId: 'session-1',
                projectDir: '/old/project',
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            } as any)
        );
        state.setGatewayClient({} as any);

        const view = new ChatView(state, bus);
        view.init();

        bus.emit('tool:result', {
            sessionId: 'session-1',
            toolName: 'ProjectManager',
            toolUseId: 'tool-1',
            result: 'Project directory changed to: D:\\git\\zero-nova\nDirectory exists: yes',
            isError: false,
        });

        const trigger = document.querySelector('.project-menu-trigger') as HTMLButtonElement;
        expect(trigger.textContent).toContain('zero-nova');
        expect(state.getSessionResourceState('session-1', 'runtime')?.data?.projectDir).toBe('D:\\git\\zero-nova');
    });
    it('shows error state when project dir is missing', async () => {
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
        expect(picker.textContent?.length).toBeGreaterThan(0);
    });

    it('uses latest gateway projectDir when refreshing Project menu', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState(
            'session-1',
            'runtime',
            state.setLoadedResource({
                sessionId: 'session-1',
                projectDir: '/old/project',
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            } as any)
        );
        state.setGatewayClient({
            getSessionRuntime: vi.fn().mockResolvedValue({
                sessionId: 'session-1',
                projectDir: '/new/project',
                totalUsage: { inputTokens: 0, outputTokens: 0 },
            }),
        } as any);

        const view = new ChatView(state, bus);
        view.init();

        const trigger = document.querySelector('.project-menu-trigger') as HTMLButtonElement;
        trigger.click();
        await flushAsync();
        const refreshBtn = document.querySelector('.project-menu-action[data-action="refresh"]') as HTMLButtonElement;
        refreshBtn.click();
        await flushAsync();

        const menuPath = document.querySelector('.project-menu-path') as HTMLElement;
        expect(menuPath.textContent).toContain('/new/project');
    });

    it('applies only latest consecutive refresh result', async () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.setGatewayClient({
            getSessionRuntime: vi
                .fn()
                .mockImplementationOnce(
                    () => new Promise((resolve) => setTimeout(() => resolve({
                        sessionId: 'session-1',
                        projectDir: '/first/project',
                        totalUsage: { inputTokens: 0, outputTokens: 0 },
                    }), 30))
                )
                .mockImplementationOnce(
                    () => new Promise((resolve) => setTimeout(() => resolve({
                        sessionId: 'session-1',
                        projectDir: '/second/project',
                        totalUsage: { inputTokens: 0, outputTokens: 0 },
                    }), 5))
                ),
        } as any);

        const view = new ChatView(state, bus);
        view.init();

        const trigger = document.querySelector('.project-menu-trigger') as HTMLButtonElement;
        trigger.click();
        await flushAsync();
        const refreshBtn = document.querySelector('.project-menu-action[data-action="refresh"]') as HTMLButtonElement;

        refreshBtn.click();
        refreshBtn.click();
        await new Promise((resolve) => setTimeout(resolve, 50));

        const menuPath = document.querySelector('.project-menu-path') as HTMLElement;
        expect(menuPath.textContent).toContain('/second/project');
    });

    it('keeps streaming assistant message last after system log', () => {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.setGatewayClient({} as any);

        const view = new ChatView(state, bus);
        view.init();

        bus.emit('token', { sessionId: 'session-1', token: 'hello ' });
        bus.emit('system:log', {
            sessionId: 'session-1',
            log: 'loop_guard_triggered session_id=session-1 reason=duplicate_tool_call_warning decision=warn',
        });
        bus.emit('token', { sessionId: 'session-1', token: 'world' });

        const messages = Array.from(document.querySelectorAll('#messages > .message'));
        expect(messages).toHaveLength(2);
        expect(messages[0].classList.contains('system')).toBe(true);
        expect(messages[1].classList.contains('assistant')).toBe(true);
        expect(messages[1].textContent).toContain('hello world');
    });
});
