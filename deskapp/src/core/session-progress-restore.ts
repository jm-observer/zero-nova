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
    console.log('[SessionRestore] Start restoring session progress', {
        requestedSessionId: sessionId,
        currentSessionId: state.currentSessionId,
        currentMessageCount: state.messages.length,
    });
    markSessionResourcesLoading(state, sessionId);

    const [messagesResult, runtimeResult, runsResult, permissionsResult] = await Promise.allSettled([
        gatewayClient.getMessages(sessionId),
        gatewayClient.getSessionRuntime(sessionId),
        gatewayClient.getSessionRuns(sessionId),
        gatewayClient.getPendingPermissions(sessionId),
    ]);

    console.log('[SessionRestore] Snapshot requests settled', {
        sessionId,
        messages: messagesResult.status,
        runtime: runtimeResult.status,
        runs: runsResult.status,
        permissions: permissionsResult.status,
    });

    const nextMessages = messagesResult.status === 'fulfilled'
        ? resolveSessionMessages(state.currentSessionId, sessionId, state.messages, messagesResult.value as Message[])
        : null;

    if (nextMessages) {
        console.log('[SessionRestore] Applying restored messages', {
            sessionId,
            restoredMessageCount: nextMessages.length,
        });
        state.setMessages(nextMessages as Message[]);
    } else if (messagesResult.status === 'rejected') {
        console.warn('[SessionRestore] Failed to restore messages', {
            sessionId,
            error: messagesResult.reason,
        });
    } else {
        console.log('[SessionRestore] Skipped applying restored messages because session changed', {
            requestedSessionId: sessionId,
            currentSessionId: state.currentSessionId,
        });
    }

    if (state.currentSessionId !== sessionId) {
        console.warn('[SessionRestore] Abort applying remaining snapshot because session changed', {
            requestedSessionId: sessionId,
            currentSessionId: state.currentSessionId,
        });
        return;
    }

    if (runtimeResult.status === 'fulfilled') {
        console.log('[SessionRestore] Runtime snapshot restored', {
            sessionId,
            totalUsage: runtimeResult.value.totalUsage,
        });
        state.updateSessionResourceState(sessionId, 'runtime', state.setLoadedResource(runtimeResult.value));
        state.updateSessionResourceState(sessionId, 'tokenUsage', state.setLoadedResource(runtimeResult.value.totalUsage));
    } else {
        console.warn('[SessionRestore] Runtime snapshot restore failed', {
            sessionId,
            error: runtimeResult.reason,
        });
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
        console.log('[SessionRestore] Run snapshot restored', {
            sessionId,
            runCount: runsResult.value.runs?.length ?? 0,
        });
        state.updateSessionResourceState(sessionId, 'runs', state.setLoadedResource(runsResult.value.runs ?? []));
    } else {
        console.warn('[SessionRestore] Run snapshot restore failed', {
            sessionId,
            error: runsResult.reason,
        });
        state.updateSessionResourceState(
            sessionId,
            'runs',
            state.toResourceError(runsResult.reason, 'Failed to restore session runs'),
        );
    }

    if (permissionsResult.status === 'fulfilled') {
        console.log('[SessionRestore] Permission snapshot restored', {
            sessionId,
            permissionCount: permissionsResult.value.length,
        });
        state.updateSessionResourceState(sessionId, 'permissions', state.setLoadedResource(permissionsResult.value));
    } else {
        console.warn('[SessionRestore] Permission snapshot restore failed', {
            sessionId,
            error: permissionsResult.reason,
        });
        state.updateSessionResourceState(
            sessionId,
            'permissions',
            state.toResourceError(permissionsResult.reason, 'Failed to restore session permissions'),
        );
    }

    console.log('[SessionRestore] Finished restoring session progress', { sessionId });
}

