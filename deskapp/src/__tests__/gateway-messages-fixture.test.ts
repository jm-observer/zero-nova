import fs from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { normalizeProgressEvent, parseInboundMessage, validateOutboundMessage } from '../gateway-messages';
import { ProtocolErrorHandler, validateEnvelope } from '../generated/generated-types';

const FIXTURE_DIR = path.resolve(__dirname, '../../../schemas/fixtures');

function loadFixture(name: string): string {
  return fs.readFileSync(path.join(FIXTURE_DIR, name), 'utf8');
}

describe('frontend fixture contract tests', () => {
  const validFixtures = [
    'welcome.json',
    'error.json',
    'chat.json',
    'chat_complete.json',
    'skill_activated.json',
    'task_status_changed.json',
    'progress_event.json',
    'agent_inspect.json',
    'workspace_restore.json',
  ];

  it.each(validFixtures)('解析有效 fixture：%s', (fixtureName) => {
    const result = parseInboundMessage(loadFixture(fixtureName), new ProtocolErrorHandler());

    expect(result.parsed).not.toBeNull();
    expect(result.parsed?.type).toBeTruthy();
    expect(result.errors).toHaveLength(0);
  });

  it('progress_event fixture 可被标准化', () => {
    const result = parseInboundMessage(loadFixture('progress_event.json'), new ProtocolErrorHandler());
    const event = normalizeProgressEvent(result.parsed?.payload);

    expect(event.type).toBeDefined();
  });

  it('有效 fixture 通过 envelope 检查', () => {
    for (const fixtureName of validFixtures) {
      const message = JSON.parse(loadFixture(fixtureName)) as { type: string; payload?: unknown };
      const hints = validateEnvelope(message, ['message-only']);

      expect(hints.filter((hint) => hint === 'missing-type' || hint === 'missing-payload')).toHaveLength(0);
    }
  });
});

describe('frontend invalid fixture handling', () => {
  const invalidFixtures = [
    'invalid_error_missing_code.json',
    'invalid_chat_missing_input.json',
    'invalid_agent_inspect_missing_session_id.json',
    'invalid_agent_inspect_missing_agent_id.json',
    'invalid_workspace_restore_missing_payload.json',
  ];

  it.each(invalidFixtures)('识别无效 fixture：%s', (fixtureName) => {
    const result = parseInboundMessage(loadFixture(fixtureName), new ProtocolErrorHandler());

    expect(result.parsed).not.toBeNull();
    expect(result.errors.length).toBeGreaterThan(0);
  });

  it('agent.inspect 缺少 sessionId 时无法通过出站校验', () => {
    const hints = validateOutboundMessage('agent.inspect', { agentId: 'agent-default' });

    expect(hints.some((hint) => hint.path.includes('sessionId'))).toBe(true);
  });

  it('agent.inspect 缺少 agentId 时无法通过出站校验', () => {
    const hints = validateOutboundMessage('agent.inspect', { sessionId: 'session-123' });

    expect(hints.some((hint) => hint.path.includes('agentId'))).toBe(true);
  });
});

describe('frontend edge cases', () => {
  it('新增可选字段不影响 chat 解析', () => {
    const extendedChat = {
      id: 'chat-003',
      type: 'chat',
      payload: {
        input: 'test',
        sessionId: '123',
        agentId: 'agent-1',
        customField: 'custom-value',
        nestedField: { key: 'value' },
      },
    };

    const result = parseInboundMessage(JSON.stringify(extendedChat), new ProtocolErrorHandler());

    expect(result.parsed?.type).toBe('chat');
  });

  it('workspace.restore 允许空 payload 对象，但不允许缺失 payload 字段', () => {
    expect(validateOutboundMessage('workspace.restore', {})).toHaveLength(0);

    const result = parseInboundMessage(
      JSON.stringify({ id: 'restore-001', type: 'workspace.restore' }),
      new ProtocolErrorHandler(),
    );

    expect(result.errors.length).toBeGreaterThan(0);
  });
});
