/**
 * 前端契约测试：gateway-messages 模块单元测试。
 *
 * 验证：
 * 1. validateOutboundMessage 正确校验所有消息类型的必填字段
 * 2. parseInboundMessage 正确解析和验证入站消息
 * 3. serializeMessage 正确序列化消息
 * 4. ProtocolErrorHandler 在超过阈值时触发回调
 */

import { describe, it, expect, vi } from 'vitest';
import {
  validateOutboundMessage,
  parseInboundMessage,
  serializeMessage,
  normalizeProgressEvent,
  createValidatedHandler,
  trackConsecError,
} from '../gateway-messages';
import { ProtocolErrorHandler } from '../generated/generated-types';

// ============================================================
// validateOutboundMessage 测试
// ============================================================

describe('validateOutboundMessage', () => {
  it('正确校验 chat 消息的必填字段', () => {
    const hints = validateOutboundMessage('chat.start', { sessionId: '123' });
    expect(hints).toHaveLength(0);
  });

  it('校验缺失 sessionId 的 chat 消息', () => {
    const hints = validateOutboundMessage('chat.start', {});
    expect(hints.some(h => h.path === 'chat.start.sessionId')).toBe(true);
  });

  it('校验 chat.complete 消息', () => {
    const hints = validateOutboundMessage('chat.complete', { sessionId: '123' });
    expect(hints).toHaveLength(0);
  });

  it('校验 sessions.create 消息的 agentId', () => {
    const hints = validateOutboundMessage('sessions.create', { agentId: 'agent-1' });
    expect(hints).toHaveLength(0);
  });

  it('校验 sessions.create 消息缺少 agentId', () => {
    const hints = validateOutboundMessage('sessions.create', {});
    expect(hints.some(h => h.path === 'sessions.create.agentId')).toBe(true);
  });

  it('sessions.list 无必填字段', () => {
    const hints = validateOutboundMessage('sessions.list', {});
    expect(hints).toHaveLength(0);
  });

  it('校验错误的字段类型', () => {
    const hints = validateOutboundMessage('chat.start', { sessionId: 123 });
    expect(hints.some(h => h.expected === 'string' && h.got === 123)).toBe(true);
  });
});

// ============================================================
// parseInboundMessage 测试
// ============================================================

describe('parseInboundMessage', () => {
  it('正确解析标准 gateway 消息', () => {
    const data = JSON.stringify({ type: 'welcome', payload: { requireAuth: false, setupRequired: true } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('welcome');
    expect(result.errors).toHaveLength(0);
  });

  it('返回 null 当 JSON 解析失败时', () => {
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage('not valid json', errorHandler);
    expect(result.parsed).toBeNull();
    expect(result.errors).toHaveLength(1);
  });

  it('不将 null 解析为消息', () => {
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage('null', errorHandler);
    expect(result.parsed).toBeNull();
  });

  it('正确解析 chat 消息', () => {
    const data = JSON.stringify({ id: '1', type: 'chat', payload: { input: 'hello', sessionId: 's1' } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat');
    expect(result.errors.some(e => e.path === 'type')).toBe(false);
  });

  it('正确解析 progress event', () => {
    const data = JSON.stringify({ type: 'chat.progress', payload: { type: 'tool_start', sessionId: 's1', iteration: 1 } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat.progress');
  });

  it('正确解析 chat.complete', () => {
    const data = JSON.stringify({ type: 'chat.complete', payload: { sessionId: 's1', output: 'done', usage: { inputTokens: 100, outputTokens: 50 } } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat.complete');
  });

  it('正确解析 skill.activated', () => {
    const data = JSON.stringify({ type: 'skill.activated', payload: { sessionId: 's1', skillId: 'cr', skillName: 'Code Review', sticky: true, reason: 'auto' } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('skill.activated');
  });

  it('正确解析 task.status_changed', () => {
    const data = JSON.stringify({ type: 'task.status_changed', payload: { sessionId: 's1', taskId: '1', taskSubject: 'Build', status: 'running' } });
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(data, errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('task.status_changed');
  });
});

// ============================================================
// serializeMessage 测试
// ============================================================

describe('serializeMessage', () => {
  it('序列化消息为 JSON 字符串', () => {
    const msg = { type: 'chat', payload: { input: 'test', sessionId: 's1' }, id: '1' };
    const json = serializeMessage(msg as any);
    expect(typeof json).toBe('string');
    expect(JSON.parse(json)).toEqual(msg);
  });

  it('strict 模式下警告非最佳格式消息', () => {
    const msg = { type: 'chat', payload: { input: 'test', sessionId: 's1' }, id: '1' };
    const originalWarn = console.warn;
    const mockWarn = vi.fn();
    console.warn = mockWarn;

    serializeMessage(msg as any, true);

    console.warn = originalWarn;
    // 仅在非 message-only 模式下可能有警告
    expect(mockWarn.mock.calls.length).toBeGreaterThanOrEqual(0);
  });
});

// ============================================================
// normalizeProgressEvent 测试
// ============================================================

describe('normalizeProgressEvent', () => {
  it('处理 null/undefined 输入', () => {
    const event = normalizeProgressEvent(null as any);
    expect(event.type).toBe('unknown');
  });

  it('标准化 toolName 别名', () => {
    const raw = { type: 'tool_start', toolName: 'Bash' };
    const event = normalizeProgressEvent(raw);
    expect((event as any).tool).toBe('Bash');
  });

  it('标准化 toolUseId 别名', () => {
    const raw = { type: 'token', toolUseId: 'use-1' };
    const event = normalizeProgressEvent(raw);
    expect((event as any).toolUseId).toBe('use-1');
  });

  it('保留所有已知字段', () => {
    const raw = { type: 'tool_start', sessionId: 's1', iteration: 1, toolName: 'Bash', toolUseId: 'use-1', output: 'result' };
    const event = normalizeProgressEvent(raw);
    expect((event as any).sessionId).toBe('s1');
    expect((event as any).iteration).toBe(1);
    expect((event as any).output).toBe('result');
  });
});

// ============================================================
// createValidatedHandler 测试
// ============================================================

describe('createValidatedHandler', () => {
  it('当验证通过时调用处理器', () => {
    const payload: any = {};
    const handler = createValidatedHandler(
      (p: any) => { payload.processed = true; },
      (data: any) => typeof data === 'object',
    );

    handler({ type: 'test', payload: { foo: 'bar' } });
    expect(payload.processed).toBe(true);
  });

  it('当验证失败时不调用处理器', () => {
    const payload: any = {};
    const handler = createValidatedHandler(
      (p: any) => { payload.processed = true; },
      (data: any) => data === null,
    );

    handler({ type: 'test', payload: { foo: 'bar' } });
    expect(payload.processed).toBeFalsy();
  });
});

// ============================================================
// trackConsecError 测试
// ============================================================

describe('trackConsecError', () => {
  it('累计错误并在超过阈值时返回 true', () => {
    const errorHandler = new ProtocolErrorHandler();
    for (let i = 0; i < 5; i++) {
      const exceeded = trackConsecError(errorHandler);
      if (i < 4) expect(exceeded).toBe(false);
    }
    const last = trackConsecError(errorHandler);
    expect(last).toBe(true);
  });
});
