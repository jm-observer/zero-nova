import { resolveSessionMessages } from './session-message-sync';
import type {
    Message,
    PermissionRequestView,
    ResourceState,
    RunSummaryView,
    SessionRuntimeSnapshot,
    TokenUsageView,
} from './types';

type SessionResourceKey = 'runtime' | 'tokenUsage' | 'runs' | 'permissions';

interface SessionProgressRestoreGateway {
    getMessages(sessionId: string): Promise<unknown[]>;
    getSessionRuntime(sessionId: string): Promise<SessionRuntimeSnapshot>;
    getSessionRuns(sessionId: string): Promise<{ runs: RunSummaryView[]; total: number }>;
    getPendingPermissions(sessionId?: string): Promise<PermissionRequestView[]>;
}

interface SessionProgressRestoreState {
    currentSessionId: string | null;
    messages: Message[];
    setMessages(messages: Message[]): void;
    getSessionResourceState(sessionId: string, key: SessionResourceKey): ResourceState<unknown> | undefined;
    updateSessionResourceState(
        sessionId: string,
        key: SessionResourceKey,
        update: Partial<ResourceState<unknown>>
    ): void;
    createEmptyResource<T>(): ResourceState<T>;
    setLoadingResource<T>(state: ResourceState<T>): ResourceState<T>;
    setLoadedResource<T>(data: T): ResourceState<T>;
    toResourceError<T>(error: unknown, fallbackMessage: string): ResourceState<T>;
}

function markSessionResourcesLoading(state: SessionProgressRestoreState, sessionId: string): void {
    const keys: SessionResourceKey[] = ['runtime', 'tokenUsage', 'runs', 'permissions'];
    for (const key of keys) {
        const current = state.getSessionResourceState(sessionId, key) ?? state.createEmptyResource();
        state.updateSessionResourceState(sessionId, key, state.setLoadingResource(current));
    }
}

export async function restoreSessionProgress(
    state: SessionProgressRestoreState,
    gatewayClient: SessionProgressRestoreGateway,
    sessionId: string
): Promise<void> {
    markSessionResourcesLoading(state, sessionId);

    const [messagesResult, runtimeResult, runsResult, permissionsResult] = await Promise.allSettled([
        gatewayClient.getMessages(sessionId),
        gatewayClient.getSessionRuntime(sessionId),
        gatewayClient.getSessionRuns(sessionId),
        gatewayClient.getPendingPermissions(sessionId),
    ]);

    const nextMessages = messagesResult.status === 'fulfilled'
        ? resolveSessionMessages(state.currentSessionId, sessionId, state.messages, messagesResult.value as Message[])
        : null;

    if (nextMessages) {
        state.setMessages(nextMessages as Message[]);
    }

    if (state.currentSessionId !== sessionId) {
        return;
    }

    if (runtimeResult.status === 'fulfilled') {
        state.updateSessionResourceState(sessionId, 'runtime', state.setLoadedResource(runtimeResult.value));
        state.updateSessionResourceState(sessionId, 'tokenUsage', state.setLoadedResource(runtimeResult.value.totalUsage));
    } else {
        state.updateSessionResourceState(
            sessionId,
            'runtime',
            state.toResourceError(runtimeResult.reason, 'Failed to restore session runtime'),
        );
        state.updateSessionResourceState(
            sessionId,
            'tokenUsage',
            state.toResourceError(runtimeResult.reason, 'Failed to restore session token usage'),
        );
    }

    if (runsResult.status === 'fulfilled') {
        state.updateSessionResourceState(sessionId, 'runs', state.setLoadedResource(runsResult.value.runs ?? []));
    } else {
        state.updateSessionResourceState(
            sessionId,
            'runs',
            state.toResourceError(runsResult.reason, 'Failed to restore session runs'),
        );
    }

    if (permissionsResult.status === 'fulfilled') {
        state.updateSessionResourceState(sessionId, 'permissions', state.setLoadedResource(permissionsResult.value));
    } else {
        state.updateSessionResourceState(
            sessionId,
            'permissions',
            state.toResourceError(permissionsResult.reason, 'Failed to restore session permissions'),
        );
    }
}
