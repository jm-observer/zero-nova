import { beforeEach, describe, expect, it, vi } from 'vitest';

import { EventBus } from '../core/event-bus';
import { AppState } from '../core/state';
import { ChatView } from '../ui/chat-view';

function flushAsync(): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, 0));
}

describe('ChatView @ tool merge (Plan-4)', () => {
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

    function createView() {
        const bus = new EventBus();
        const state = new AppState(bus);
        state.setCurrentSession('session-1');
        state.updateSessionResourceState('session-1', 'runtime', state.setLoadedResource({ sessionId: 'session-1', totalUsage: { inputTokens: 0, outputTokens: 0 } }));
        state.setGatewayClient({} as any);

        const view = new ChatView(state, bus) as ChatView & {
            streamingMessageEl: HTMLElement | null;
            pendingToolResults: Map<string, Map<string, any>>;
            lastSessionId: string | null;
        };
        view.init();
        return { view, bus, state };
    }

    describe('handleToolStart', () => {
        it('should create a tool-use-card with tool-result-container', async () => {
            const { view, bus } = createView();
            const input = document.getElementById('message-input') as HTMLTextAreaElement;
            
            bus.emit('tool:start', {
                toolName: 'readFile',
                args: { path: 'src/main.ts' },
                toolUseId: 'tool-1',
                sessionId: 'session-1'
            });

            // 等待 streaming message 创建
            await flushAsync();
            
            const card = document.querySelector('.tool-use-card[data-tool-use-id="tool-1"]');
            expect(card).toBeTruthy();
            expect(card?.querySelector('.tool-result-container')).toBeTruthy();
        });

        it('should use resolved toolUseId when event has both id and toolUseId', async () => {
            const { view, bus } = createView();
            
            bus.emit('tool:start', {
                toolName: 'writeFile',
                args: {},
                id: 'resolved-id',
                toolUseId: 'old-id',
                sessionId: 'session-1'
            });

            await flushAsync();
            
            const card = document.querySelector('.tool-use-card[data-tool-use-id="resolved-id"]');
            expect(card).toBeTruthy();
        });
    });

    describe('handleToolResult', () => {
        it('should cache result when streamingMessageEl is missing', async () => {
            const { view, bus } = createView();
            
            // 强制移除 streamingMessageEl
            (view as any).streamingMessageEl = null;
            
            bus.emit('tool:result', {
                toolUseId: 'tool-1',
                result: '{"content": "hello"}',
                isError: false,
                sessionId: 'session-1'
            });

            // 验证缓存
            const pendingResults = (view as any).pendingToolResults;
            expect(pendingResults.has('session-1')).toBeTruthy();
            expect(pendingResults.get('session-1')?.has('tool-1')).toBeTruthy();
        });

        it('should render cached result when tool start arrives', async () => {
            const { view, bus } = createView();
            
            // 先缓存结果
            const pendingResults = (view as any).pendingToolResults;
            pendingResults.set('session-1', new Map([
                ['tool-1', { result: '{"content": "hello"}', isError: false, sessionId: 'session-1' }]
            ]));

            // 再创建 tool-use-card
            bus.emit('tool:start', {
                toolName: 'readFile',
                args: {},
                toolUseId: 'tool-1',
                sessionId: 'session-1'
            });

            await flushAsync();
            
            const container = document.querySelector('.tool-result-container[data-rel-id="tool-1"]');
            expect(container?.querySelector('.tool-result-card')).toBeTruthy();
        });

        it('should use resolveToolUseId for ID compatibility', async () => {
            const { view, bus } = createView();
            
            bus.emit('tool:result', {
                tool_use_id: 'tool-1',
                result: '{"output_summary": "done"}',
                isError: false,
                sessionId: 'session-1'
            });

            const pendingResults = (view as any).pendingToolResults;
            expect(pendingResults.get('session-1')?.has('tool-1')).toBeTruthy();
        });
    });

    describe('消息恢复渲染', () => {
        it('should produce nested DOM through public render entry (MESSAGES_UPDATED)', async () => {
            const { view, bus } = createView();
            
            // 不直接调用 private buildToolHtml，改为触发公开渲染流程
            bus.emit('chat:complete', { sessionId: 'session-1' });
            
            bus.emit('messages_updated', {
                sessionId: 'session-1',
                messages: [{
                    id: 'msg-1',
                    role: 'assistant',
                    content: [
                        { type: 'tool_use', id: 'tool-1', name: 'readFile', args: {} },
                        { type: 'tool_result', id: 'tool-1', content: '{"content": "hello"}' }
                    ],
                    createdAt: Date.now()
                }]
            });

            await flushAsync();
            
            const toolUseCard = document.querySelector('.tool-use-card[data-tool-use-id="tool-1"]');
            expect(toolUseCard).toBeTruthy();
        });
    });

    describe('跨会话缓存', () => {
        it('different sessions with same toolUseId should not interfere', async () => {
            const { view, bus } = createView();
            
            (view as any).streamingMessageEl = null;
            
            // 会话 1 的结果
            bus.emit('tool:result', {
                toolUseId: 'tool-1',
                result: '{"content": "session1"}',
                isError: false,
                sessionId: 'session-1'
            });
            
            // 会话 2 的结果（相同 toolUseId）
            bus.emit('tool:result', {
                toolUseId: 'tool-1',
                result: '{"content": "session2"}',
                isError: false,
                sessionId: 'session-2'
            });
            
            // 验证两个会话都有缓存
            const pendingResults = (view as any).pendingToolResults;
            expect(pendingResults.has('session-1')).toBeTruthy();
            expect(pendingResults.has('session-2')).toBeTruthy();
        });
    });
});
