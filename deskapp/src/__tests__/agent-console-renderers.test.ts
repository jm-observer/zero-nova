import { describe, expect, it } from 'vitest';

import { buildOverviewSummary } from '../ui/agent-console-renderers';

describe('buildOverviewSummary', () => {
  it('??????? runtime/tools/skills ??????', () => {
    const summary = buildOverviewSummary({
      agent: {
        agentId: 'agent-1',
        name: 'Agent',
        model: { provider: 'openai', model: 'gpt-4o-mini', source: 'agent' },
        systemPrompt: 'test',
        activeSkills: ['skill-fallback'],
        availableTools: ['fallback-tool'],
        skills: [{ id: 'skill-fallback', title: 'Fallback', enabled: true }],
        capabilityPolicy: {},
      },
      runtime: {
        sessionId: 'session-1',
        orchestrationDetail: { provider: 'openai', model: 'gpt-5', source: 'session_override', editableScopes: ['global', 'agent', 'session_override'] },
        executionDetail: { provider: 'openai', model: 'gpt-5-mini', source: 'session_override', editableScopes: ['global', 'agent', 'session_override'] },
        totalUsage: { inputTokens: 1, outputTokens: 2 },
      },
      usage: { inputTokens: 120, outputTokens: 30 },
      tools: [
        {
          name: 'tool-a',
          description: 'A',
          source: 'builtin',
          inputSchema: {},
          enabled: true,
        },
        {
          name: 'tool-b',
          description: 'B',
          source: 'manual',
          inputSchema: {},
          enabled: true,
        },
      ],
      skillBindings: [
        { id: 'skill-a', title: 'Skill A', source: 'runtime', enabled: true },
        { id: 'skill-b', title: 'Skill B', source: 'agent', enabled: true },
      ],
    });

    expect(summary).toEqual({
      modelName: 'gpt-5',
      tokensTotal: '150',
      toolsCount: '2',
      skillsCount: '2',
    });
  });

  it('???????????? agent ????????', () => {
    const summary = buildOverviewSummary({
      agent: {
        agentId: 'agent-1',
        name: 'Agent',
        model: { provider: 'openai', model: 'gpt-4.1', source: 'agent' },
        systemPrompt: 'test',
        activeSkills: ['skill-a', 'skill-b'],
        availableTools: ['tool-a', 'tool-b', 'tool-c'],
        skills: [
          { id: 'skill-a', title: 'Skill A', enabled: true },
          { id: 'skill-b', title: 'Skill B', enabled: true },
        ],
        capabilityPolicy: {},
      },
      usage: { inputTokens: 10, outputTokens: 5 },
    });

    expect(summary).toEqual({
      modelName: 'gpt-4.1',
      tokensTotal: '15',
      toolsCount: '3',
      skillsCount: '2',
    });
  });
});
