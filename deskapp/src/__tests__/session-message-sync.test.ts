import { describe, expect, it } from 'vitest';

import { resolveSessionMessages } from '../core/session-message-sync';
import type { Message } from '../core/types';

describe('resolveSessionMessages', () => {
    it('returns null for stale session responses', () => {
        const result = resolveSessionMessages('session-2', 'session-1', [], []);

        expect(result).toBeNull();
    });

    it('merges local optimistic messages that are not yet persisted', () => {
        const loadedMessages: Message[] = [
            { id: 'user-1', role: 'user', content: 'hello', createdAt: 1 },
            { id: 'assistant-1', role: 'assistant', content: 'world', createdAt: 2 },
        ];
        const localMessages: Message[] = [
            ...loadedMessages,
            { id: 'tmp-1', role: 'user', content: 'pending', createdAt: 3 },
        ];

        const result = resolveSessionMessages('session-1', 'session-1', localMessages, loadedMessages);

        expect(result).toEqual([
            ...loadedMessages,
            { id: 'tmp-1', role: 'user', content: 'pending', createdAt: 3 },
        ]);
    });

    it('does not duplicate optimistic messages already returned by the server', () => {
        const localMessages: Message[] = [
            { id: 'tmp-1', role: 'user', content: 'pending', createdAt: 1 },
        ];
        const loadedMessages: Message[] = [
            { id: 'user-1', role: 'user', content: 'pending', createdAt: 2 },
        ];

        const result = resolveSessionMessages('session-1', 'session-1', localMessages, loadedMessages);

        expect(result).toEqual(loadedMessages);
    });
});
