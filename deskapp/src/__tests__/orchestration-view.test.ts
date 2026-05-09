import { describe, expect, it } from 'vitest';

import { EventBus } from '../core/event-bus';
import { OrchestrationView } from '../ui/orchestration-view';

describe('OrchestrationView', () => {
    it('consumes camelCase orchestration payloads', () => {
        const bus = new EventBus();
        const container = document.createElement('div');
        document.body.appendChild(container);
        const view = new OrchestrationView(bus, container, () => 'session-1');

        expect(view).toBeTruthy();

        bus.emit('orchestration:plan', {
            sessionId: 'session-1',
            planId: 'plan-1',
            description: '编排测试',
            stages: [
                {
                    stageId: 's1',
                    mode: 'parallel',
                    dependsOn: [],
                    agents: [
                        {
                            agentId: 'agent-1',
                            description: '实现协议',
                            subagentType: 'Coder',
                        },
                    ],
                },
            ],
        });

        bus.emit('orchestration:agent_spawn', {
            sessionId: 'session-1',
            planId: 'plan-1',
            agentId: 'agent-1',
            stageId: 's1',
        });
        bus.emit('orchestration:agent_complete', {
            sessionId: 'session-1',
            planId: 'plan-1',
            agentId: 'agent-1',
            stageId: 's1',
            status: 'success',
            outputSummary: '完成',
        });
        bus.emit('orchestration:stage_complete', {
            sessionId: 'session-1',
            planId: 'plan-1',
            stageId: 's1',
            mode: 'parallel',
            allSuccess: true,
        });
        bus.emit('orchestration:review_start', {
            sessionId: 'session-1',
            planId: 'plan-1',
        });
        bus.emit('orchestration:complete', {
            sessionId: 'session-1',
            planId: 'plan-1',
            overallSuccess: true,
            summary: '全部完成',
        });

        expect(container.querySelector('#plan-plan-1')).not.toBeNull();
        expect(container.querySelector('#stage-plan-1-s1')?.classList.contains('status-completed')).toBe(true);
        expect(container.querySelector('#agent-card-plan-1-agent-1')?.classList.contains('status-success')).toBe(true);
        expect(container.querySelector('.plan-progress-text')?.textContent).toBe('1 / 1');
        expect(container.querySelector('.plan-status-badge')?.textContent).toBe('完成');

        container.remove();
    });

    it('isolates agent updates across plans and shows failure summary', () => {
        const bus = new EventBus();
        const container = document.createElement('div');
        document.body.appendChild(container);
        new OrchestrationView(bus, container, () => 'session-1');

        for (const planId of ['plan-1', 'plan-2']) {
            bus.emit('orchestration:plan', {
                sessionId: 'session-1',
                planId,
                description: `计划 ${planId}`,
                stages: [
                    {
                        stageId: 's1',
                        mode: 'parallel',
                        dependsOn: [],
                        agents: [
                            {
                                agentId: 'a1',
                                description: `任务 ${planId}`,
                                subagentType: 'Coder',
                            },
                        ],
                    },
                ],
            });
        }

        bus.emit('orchestration:agent_complete', {
            sessionId: 'session-1',
            planId: 'plan-2',
            agentId: 'a1',
            stageId: 's1',
            status: 'failed',
            error: 'network timeout',
        });

        expect(container.querySelector('#agent-card-plan-1-a1')?.classList.contains('status-failed')).toBe(false);
        expect(container.querySelector('#agent-card-plan-2-a1')?.classList.contains('status-failed')).toBe(true);
        expect(container.querySelector('#summary-plan-2-a1')?.textContent).toContain('network timeout');

        container.remove();
    });
});

