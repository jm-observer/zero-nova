import { afterEach, describe, expect, it, vi } from 'vitest';

import { GatewayClient } from '../gateway-client';

describe('GatewayClient contract guards', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('无效的 agent.inspect payload 不会进入 ws.send', async () => {
    const client = new GatewayClient('ws://localhost:3000');
    const send = vi.fn();

    (client as unknown as { ws: { readyState: number; send: typeof send } }).ws = {
      readyState: WebSocket.OPEN,
      send,
    };

    await expect(
      client.request('agent.inspect', { sessionId: 'session-123' }, 0),
    ).rejects.toThrow('outbound message validation failed');
    expect(send).not.toHaveBeenCalled();
  });

  it('合法的 workspace.restore 请求会保留 payload 外层包装', () => {
    const client = new GatewayClient('ws://localhost:3000');
    const send = vi.fn();

    (client as unknown as { ws: { readyState: number; send: typeof send } }).ws = {
      readyState: WebSocket.OPEN,
      send,
    };

    client.request('workspace.restore', {}, 0);

    expect(send).toHaveBeenCalledTimes(1);
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({
      type: 'workspace.restore',
      payload: {},
    });
  });

  it('合法的 agent.inspect 请求会序列化完整 payload', () => {
    const client = new GatewayClient('ws://localhost:3000');
    const send = vi.fn();

    (client as unknown as { ws: { readyState: number; send: typeof send } }).ws = {
      readyState: WebSocket.OPEN,
      send,
    };

    client.request(
      'agent.inspect',
      { sessionId: 'session-123', agentId: 'agent-default' },
      0,
    );

    expect(send).toHaveBeenCalledTimes(1);
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({
      type: 'agent.inspect',
      payload: { sessionId: 'session-123', agentId: 'agent-default' },
    });
  });
});
