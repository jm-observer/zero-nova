import { describe, expect, it } from 'vitest';

import { EventBus } from '../core/event-bus';
import { AppState } from '../core/state';

describe('AppState voice helpers', () => {
  it('更新语音状态并同步转写临时消息', () => {
    const state = new AppState(new EventBus());

    state.updateVoiceConversation({
      active: true,
      phase: 'recognizing',
      transcriptState: 'pending',
    });
    state.upsertVoiceTranscriptMessage('voice-1', '', 'pending');
    state.upsertVoiceTranscriptMessage('voice-1', '你好，世界', 'final');

    expect(state.voiceConversation.phase).toBe('recognizing');
    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].content).toBe('你好，世界');
    expect(state.messages[0].metadata?.voiceTranscriptState).toBe('final');
  });
});
