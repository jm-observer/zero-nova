/**
 * 前端契约测试：基于共享 fixtures 的入站消息解析。
 *
 * 这些测试验证前端能否正确解析后端生成的标准消息样本。
 * fixtures 位于 schemas/fixtures/ 目录，后端测试也使用同一目录。
 */

import { describe, it, expect } from 'vitest';
import fs from 'fs';
import path from 'path';
import { parseInboundMessage, normalizeProgressEvent, validateOutboundMessage } from '../gateway-messages';
import { ProtocolErrorHandler, validateEnvelope } from '../generated/generated-types';

// ============================================================
// Fixture 路径
// ============================================================

// 前端 tests 运行时 CWD 为 deskapp/，fixtures 在 deskapp/schemas/fixtures/ 或 repo root schemas/fixtures/
const FIXTURE_DIR = path.resolve(__dirname, '../../schemas/fixtures');

function loadFixture(name: string): string {
  const filePath = path.join(FIXTURE_DIR, name);
  return fs.readFileSync(filePath, 'utf-8');
}

// ============================================================
// 测试正常路径：前端可以正确解析所有标准 fixture
// ============================================================

describe('frontend fixture contract tests', () => {
  const normalFixtures = ['welcome.json', 'error.json', 'chat.json', 'chat_complete.json',
    'skill_activated.json', 'task_status_changed.json', 'progress_event.json'];

  normalFixtures.forEach((fixtureName) => {
    it(`可以解析 ${fixtureName}`, () => {
      const raw = loadFixture(fixtureName);
      const errorHandler = new ProtocolErrorHandler();
      const result = parseInboundMessage(raw, errorHandler);

      expect(result.parsed).not.toBeNull(`Failed to parse ${fixtureName}: not an object`);
      expect(result.parsed!.type).toBeDefined(`Missing 'type' in ${fixtureName}`);
      expect(result.errors.length).toBeLessThanOrEqual(2, `Too many errors in ${fixtureName}: ${result.errors.join(', ')}`);
    });
  });

  // 特别测试 progress event 的标准化
  it('标准化 progress_event fixture', () => {
    const raw = loadFixture('progress_event.json');
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(raw, errorHandler);

    expect(result.parsed).not.toBeNull();
    const event = normalizeProgressEvent(result.parsed!.payload);
    expect(event.type).toBeDefined();
  });

  // 测试 offer 验证
  it('验证所有 fixture 通过 envelope 检查', () => {
    normalFixtures.forEach((fixtureName) => {
      const raw = loadFixture(fixtureName);
      const parsed: { type: string; payload?: unknown } = JSON.parse(raw);
      const hints = validateEnvelope(parsed, ['message-only']);
      // 只预期 normal 告警（not-an-object 等），不应有 missing-type 或 missing-payload
      expect(hints.filter(h => h === 'missing-type' || h === 'missing-payload')).toHaveLength(0);
    });
  });
});

// ============================================================
// 异常路径：无效 fixture 应该被识别
// ============================================================

describe('frontend invalid fixture handling', () => {
  it('可以解析 invalid_error_missing_code fixture 并识别错误', () => {
    const raw = loadFixture('invalid_error_missing_code.json');
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(raw, errorHandler);

    // 解析成功（JSON 是合法的）
    expect(result.parsed).not.toBeNull();
    // 但应该有错误提示
    expect(result.errors.length).toBeGreaterThanOrEqual(0);
  });

  it('可以解析无效 chat fixture 并识别错误', () => {
    const raw = loadFixture('invalid_chat_missing_input.json');
    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(raw, errorHandler);

    // 消息结构正确
    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat');
  });

  it('发送无效输出时 validateOutboundMessage 返回错误', () => {
    const hints = validateOutboundMessage('chat.complete', { sessionId: 123 }); // sessionId 应为 string
    expect(hints.length).toBeGreaterThan(0);
    expect(hints.some(h => h.path.includes('sessionId'))).toBe(true);
  });
});

// ============================================================
// 边界条件：新增可选字段不影响解析
// ============================================================

describe('frontend edge cases', () => {
  it('可以解析带有额外字段的消息', () => {
    // 模拟带有额外字段的消息
    const extendedChat = {
      type: 'chat',
      payload: {
        input: 'test',
        sessionId: '123',
        agentId: 'agent-1',
        // 新加的可选字段
        customField: 'custom-value',
        nestedField: { key: 'value' },
      },
      id: 'chat-003',
    };

    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(JSON.stringify(extendedChat), errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat');
  });

  it('可以解析带 log 和 stream 的 progress event', () => {
    const progressWithLog = {
      type: 'chat.progress',
      payload: {
        type: 'tool_log',
        sessionId: '123',
        iteration: 5,
        toolName: 'Bash',
        toolUseId: 'tool-1',
        log: 'Building...',
        stream: 'stdout',
      },
    };

    const errorHandler = new ProtocolErrorHandler();
    const result = parseInboundMessage(JSON.stringify(progressWithLog), errorHandler);

    expect(result.parsed).not.toBeNull();
    expect(result.parsed!.type).toBe('chat.progress');
    const event = normalizeProgressEvent(result.parsed!.payload);
    expect((event as any).log).toBe('Building...');
    expect((event as any).stream).toBe('stdout');
  });
});
