import { afterEach, describe, expect, it, vi } from 'vitest';

import { GatewayClient, GatewayRequestError } from '../gateway-client';

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


  it('getAgentInspect ?????????????', async () => {
    const client = new GatewayClient('ws://localhost:3000');

    vi.spyOn(client, 'request').mockResolvedValue({
      agentId: 'agent-default',
      name: 'Default Agent',
      model: { provider: 'openai', model: 'gpt-4.1', source: 'global' },
      systemPrompt: 'prompt',
      capabilityPolicy: {},
    } as Awaited<ReturnType<typeof client.getAgentInspect>>);

    await expect(
      client.getAgentInspect({ sessionId: 'session-123', agentId: 'agent-default' }),
    ).resolves.toMatchObject({
      activeSkills: [],
      availableTools: [],
      skills: [],
    });
  });

  it('getVoiceCapabilities ???????????????', async () => {
    const client = new GatewayClient('ws://localhost:3000');

    vi.spyOn(client, 'request').mockRejectedValue(
      new GatewayRequestError('Not implemented', { code: 'not_implemented' }),
    );

    await expect(client.getVoiceCapabilities()).resolves.toEqual({
      stt: { enabled: false, available: false },
      tts: { enabled: false, available: false, voice: '', autoPlay: false },
    });
  });

  it('getSessionRuns 会归一化运行模型和工具调用数', async () => {
    const client = new GatewayClient('ws://localhost:3000');

    vi.spyOn(client, 'request').mockResolvedValue({
      runs: [
        {
          runId: 'run-1',
          sessionId: 'session-1',
          status: 'success',
          startedAt: 100,
          finishedAt: 180,
          durationMs: 80,
          orchestrationModel: { provider: 'openai', model: 'gpt-5' },
          executionModel: { provider: 'openai', model: 'gpt-5-mini' },
          toolCallCount: 3,
          usage: { inputTokens: 10, outputTokens: 5 },
        },
      ],
      total: 1,
    });

    await expect(client.getSessionRuns('session-1')).resolves.toEqual({
      runs: [
        expect.objectContaining({
          id: 'run-1',
          sessionId: 'session-1',
          status: 'completed',
          modelSummary: 'gpt-5 / gpt-5-mini',
          toolCount: 3,
          tokenUsage: {
            inputTokens: 10,
            outputTokens: 5,
            cacheCreationInputTokens: undefined,
            cacheReadInputTokens: undefined,
          },
        }),
      ],
      total: 1,
    });
  });

});
