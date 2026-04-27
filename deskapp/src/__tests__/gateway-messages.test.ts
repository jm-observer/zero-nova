import { describe, expect, it, vi } from 'vitest';

import {
  createValidatedHandler,
  normalizeProgressEvent,
  parseInboundMessage,
  serializeMessage,
  trackConsecError,
  validateOutboundMessage,
} from '../gateway-messages';
import { ProtocolErrorHandler } from '../generated/generated-types';

describe('validateOutboundMessage', () => {
  it('接受合法的 agent.inspect 请求', () => {
    expect(
      validateOutboundMessage('agent.inspect', {
        sessionId: 'session-123',
        agentId: 'agent-default',
      }),
    ).toHaveLength(0);
  });

  it('拒绝缺少 sessionId 的 agent.inspect 请求', () => {
    const hints = validateOutboundMessage('agent.inspect', { agentId: 'agent-default' });

    expect(hints.some((hint) => hint.path.includes('sessionId'))).toBe(true);
  });

  it('拒绝缺少 agentId 的 agent.inspect 请求', () => {
    const hints = validateOutboundMessage('agent.inspect', { sessionId: 'session-123' });

    expect(hints.some((hint) => hint.path.includes('agentId'))).toBe(true);
  });

  it('要求 workspace.restore 保留 payload 包装', () => {
    expect(validateOutboundMessage('workspace.restore', {})).toHaveLength(0);
  });

  it('继续校验已有自定义出站消息', () => {
    const hints = validateOutboundMessage('chat.complete', { sessionId: 123 });

    expect(hints.some((hint) => hint.path.includes('sessionId'))).toBe(true);
  });
});

describe('parseInboundMessage', () => {
  it('解析标准入站消息', () => {
    const raw = JSON.stringify({
      type: 'welcome',
      payload: { requireAuth: false, setupRequired: true },
    });

    const result = parseInboundMessage(raw, new ProtocolErrorHandler());

    expect(result.parsed?.type).toBe('welcome');
    expect(result.errors).toHaveLength(0);
  });

  it('拒绝非法 JSON', () => {
    const result = parseInboundMessage('not valid json', new ProtocolErrorHandler());

    expect(result.parsed).toBeNull();
    expect(result.errors).toHaveLength(1);
  });

  it('对缺少 payload 的 workspace.restore 产生错误', () => {
    const raw = JSON.stringify({ id: 'restore-001', type: 'workspace.restore' });

    const result = parseInboundMessage(raw, new ProtocolErrorHandler());

    expect(result.parsed?.type).toBe('workspace.restore');
    expect(result.errors.length).toBeGreaterThan(0);
  });
});

describe('serializeMessage', () => {
  it('保留外层 payload 包装', () => {
    const message = {
      id: 'inspect-001',
      type: 'agent.inspect',
      payload: { sessionId: 'session-123', agentId: 'agent-default' },
    };

    expect(JSON.parse(serializeMessage(message))).toEqual(message);
  });

  it('strict 模式下对非标准 envelope 发出警告', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    serializeMessage({ type: 'workspace.restore' }, true);

    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});

describe('normalizeProgressEvent', () => {
  it('补齐 tool 与 toolName 别名', () => {
    const event = normalizeProgressEvent({ type: 'tool_start', toolName: 'Bash' });

    expect(event.tool).toBe('Bash');
    expect(event.toolName).toBe('Bash');
  });

  it('补齐 toolUseId 与 tool_use_id 别名', () => {
    const event = normalizeProgressEvent({ type: 'tool_start', tool_use_id: 'use-1' });

    expect(event.toolUseId).toBe('use-1');
    expect(event.tool_use_id).toBe('use-1');
  });
});

describe('createValidatedHandler', () => {
  it('仅在校验通过时调用处理器', () => {
    const handler = vi.fn();
    const validated = createValidatedHandler(
      handler,
      (payload: unknown): payload is { sessionId: string } =>
        typeof payload === 'object' && payload !== null && typeof (payload as { sessionId?: unknown }).sessionId === 'string',
    );

    validated({ type: 'chat.start', payload: { sessionId: 'session-123' } });
    validated({ type: 'chat.start', payload: { sessionId: 1 } });

    expect(handler).toHaveBeenCalledTimes(1);
  });
});

describe('trackConsecError', () => {
  it('在超过阈值后返回 true', () => {
    const errorHandler = new ProtocolErrorHandler();

    for (let index = 0; index < 5; index += 1) {
      expect(trackConsecError(errorHandler)).toBe(false);
    }

    expect(trackConsecError(errorHandler)).toBe(true);
  });
});
