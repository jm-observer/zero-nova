import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { EventBus } from '../core/event-bus';
import { ModalsView } from '../ui/modals';

describe('ModalsView', () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <div id="confirm-modal" class="modal hidden"></div>
      <div id="file-preview-modal" class="modal hidden"></div>
      <div id="tool-detail-modal" class="modal hidden"></div>
    `;
  });

  it('在仅有根节点时仍能完成初始化并补齐内部结构', () => {
    const view = new ModalsView(new EventBus());

    expect(() => view.init()).not.toThrow();
    expect(document.getElementById('confirm-yes')).not.toBeNull();
    expect(document.getElementById('file-preview-close')).not.toBeNull();
    expect(document.getElementById('tool-detail-close')).not.toBeNull();
  });
});
