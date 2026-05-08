import { describe, expect, it, vi } from 'vitest';

import { restoreSessionProgress } from '../core/session-progress-restore';
import type { Message, ResourceState, SessionRuntimeSnapshot, TokenUsageView } from '../core/types';

type SessionResourceKey = 'runtime' | 'tokenUsage' | 'runs' | 'permissions';

function createState(sessionId: string | null, messages: Message[] = []) {
    const resources = new Map<string, Partial<Record<SessionResourceKey, ResourceState<unknown>>>>();

    return {
        currentSessionId: sessionId,
        messages,
        setMessages: vi.fn((nextMessages: Message[]) => {
            messages.splice(0, messages.length, ...nextMessages);
        }),
        getSessionResourceState: (targetSessionId: string, key: SessionResourceKey) => resources.get(targetSessionId)?.[key],
        updateSessionResourceState: vi.fn((targetSessionId: string, key: SessionResourceKey, update: Partial<ResourceState<unknown>>) => {
            const sessionResources = resources.get(targetSessionId) ?? {};
            const current = sessionResources[key] ?? { status: 'idle', loaded: false, loading: false, unsupported: false };
            sessionResources[key] = { ...current, ...update };
            resources.set(targetSessionId, sessionResources);
        }),
        createEmptyResource<T>(): ResourceState<T> {
            return { status: 'idle', loaded: false, loading: false, unsupported: false };
        },
        setLoadingResource<T>(state: ResourceState<T>): ResourceState<T> {
            return { ...state, status: 'loading', loading: true, error: undefined, unsupported: false };
        },
        setLoadedResource<T>(data: T): ResourceState<T> {
            return { status: 'ready', loaded: true, loading: false, data, unsupported: false, updatedAt: Date.now() };
        },
        toResourceError<T>(error: unknown, fallbackMessage: string): ResourceState<T> {
            return {
                status: 'error',
                loaded: true,
                loading: false,
                unsupported: false,
                error: error instanceof Error ? error.message : fallbackMessage,
                updatedAt: Date.now(),
            };
        },
        resources,
    };
}

describe('restoreSessionProgress', () => {
    it('恢复当前 Session 的消息、运行态、runs 和权限快照', async () => {
        const state = createState('session-1');
        const runtime: SessionRuntimeSnapshot = {
            sessionId: 'session-1',
            totalUsage: { inputTokens: 12, outputTokens: 8 },
        };
        const gatewayClient = {
            getMessages: vi.fn().mockResolvedValue([
                { id: 'assistant-1', role: 'assistant', content: 'done', createdAt: 1 },
            ]),
            getSessionRuntime: vi.fn().mockResolvedValue(runtime),
            getSessionRuns: vi.fn().mockResolvedValue({
                runs: [{ id: 'run-1', sessionId: 'session-1', status: 'running', startedAt: 1, toolCount: 0 }],
                total: 1,
            }),
            getPendingPermissions: vi.fn().mockResolvedValue([
                { id: 'perm-1', sessionId: 'session-1', status: 'pending' },
            ]),
        } as any;

        await restoreSessionProgress(state as any, gatewayClient, 'session-1');

        expect(state.setMessages).toHaveBeenCalledWith([
            { id: 'assistant-1', role: 'assistant', content: 'done', createdAt: 1 },
        ]);
        expect(state.resources.get('session-1')?.runtime?.data).toEqual(runtime);
        expect(state.resources.get('session-1')?.tokenUsage?.data).toEqual(runtime.totalUsage as TokenUsageView);
        expect(state.resources.get('session-1')?.runs?.data).toEqual([
            { id: 'run-1', sessionId: 'session-1', status: 'running', startedAt: 1, toolCount: 0 },
        ]);
        expect(state.resources.get('session-1')?.permissions?.data).toEqual([
            { id: 'perm-1', sessionId: 'session-1', status: 'pending' },
        ]);
    });

    it('会忽略切换会话后的过期恢复结果', async () => {
        const state = createState('session-1');

        let resolveMessages: ((value: unknown[]) => void) | undefined;
        let resolveRuntime: ((value: SessionRuntimeSnapshot) => void) | undefined;
        let resolveRuns: ((value: { runs: unknown[]; total: number }) => void) | undefined;
        let resolvePermissions: ((value: unknown[]) => void) | undefined;

        const gatewayClient = {
            getMessages: vi.fn(() => new Promise(resolve => {
                resolveMessages = resolve;
            })),
            getSessionRuntime: vi.fn(() => new Promise(resolve => {
                resolveRuntime = resolve;
            })),
            getSessionRuns: vi.fn(() => new Promise(resolve => {
                resolveRuns = resolve;
            })),
            getPendingPermissions: vi.fn(() => new Promise(resolve => {
                resolvePermissions = resolve;
            })),
        } as any;

        const pending = restoreSessionProgress(state as any, gatewayClient, 'session-1');
        state.currentSessionId = 'session-2';

        resolveMessages?.([{ id: 'assistant-1', role: 'assistant', content: 'done', createdAt: 1 }]);
        resolveRuntime?.({ sessionId: 'session-1', totalUsage: { inputTokens: 1, outputTokens: 1 } });
        resolveRuns?.({ runs: [{ id: 'run-1' }], total: 1 });
        resolvePermissions?.([{ id: 'perm-1' }]);
        await pending;

        expect(state.setMessages).not.toHaveBeenCalled();
        expect(state.resources.get('session-1')?.runtime?.status).toBe('loading');
        expect(state.resources.get('session-1')?.runs?.status).toBe('loading');
        expect(state.resources.get('session-1')?.permissions?.status).toBe('loading');
    });

    it('运行态恢复失败时会给 runtime 和 tokenUsage 标记错误', async () => {
        const state = createState('session-1');
        const gatewayClient = {
            getMessages: vi.fn().mockResolvedValue([]),
            getSessionRuntime: vi.fn().mockRejectedValue(new Error('runtime failed')),
            getSessionRuns: vi.fn().mockResolvedValue({ runs: [], total: 0 }),
            getPendingPermissions: vi.fn().mockResolvedValue([]),
        } as any;

        await restoreSessionProgress(state as any, gatewayClient, 'session-1');

        expect(state.resources.get('session-1')?.runtime?.status).toBe('error');
        expect(state.resources.get('session-1')?.runtime?.error).toBe('runtime failed');
        expect(state.resources.get('session-1')?.tokenUsage?.status).toBe('error');
        expect(state.resources.get('session-1')?.tokenUsage?.error).toBe('runtime failed');
    });
});
