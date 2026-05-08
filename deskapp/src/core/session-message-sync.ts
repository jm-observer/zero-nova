import type { Message } from './types';

export function resolveSessionMessages(
    currentSessionId: string | null,
    requestedSessionId: string,
    localMessages: Message[],
    loadedMessages: Message[],
): Message[] | null {
    if (currentSessionId !== requestedSessionId) {
        return null;
    }

    const localTmpMsgs = localMessages.filter(message => String(message.id).startsWith('tmp-'));
    if (localTmpMsgs.length === 0) {
        return loadedMessages;
    }

    const merged = [...loadedMessages];
    for (const tmpMsg of localTmpMsgs) {
        const duplicated = loadedMessages.some(serverMsg =>
            serverMsg.role === tmpMsg.role &&
            String(serverMsg.content ?? '') === String(tmpMsg.content ?? ''),
        );
        if (!duplicated) {
            merged.push(tmpMsg);
        }
    }
    return merged;
}
