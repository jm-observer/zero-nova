import { beforeEach, describe, expect, it } from 'vitest';

import { EventBus } from '../core/event-bus';
import { VoiceOverlayView } from '../ui/voice-overlay';

describe('VoiceOverlayView', () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="voice-overlay" class="voice-overlay hidden"></div>';
  });

  it('在只有 overlay 根节点时仍可初始化并补齐关闭按钮', () => {
    const view = new VoiceOverlayView(new EventBus());

    expect(() => view.init()).not.toThrow();
    expect(document.getElementById('voice-overlay-close')).not.toBeNull();
    expect(document.getElementById('voice-main-btn')).not.toBeNull();
  });

  it('收到语音状态事件后更新文案、转写和错误态', () => {
    const bus = new EventBus();
    const view = new VoiceOverlayView(bus);

    view.init();
    bus.emit('voice:state', {
      active: true,
      phase: 'error',
      transcript: '测试转写',
      transcriptState: 'final',
      error: '识别失败',
      durationSeconds: 3,
      canRetry: true,
    });

    const overlay = document.getElementById('voice-overlay');
    const transcript = document.getElementById('voice-transcript');
    const error = document.getElementById('voice-error-text');
    const retry = document.getElementById('voice-retry-btn');

    expect(overlay?.getAttribute('data-voice-phase')).toBe('error');
    expect(transcript?.textContent).toBe('测试转写');
    expect(error?.textContent).toBe('识别失败');
    expect(retry?.classList.contains('hidden')).toBe(false);
  });
});
