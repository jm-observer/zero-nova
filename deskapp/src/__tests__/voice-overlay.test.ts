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
  });
});
