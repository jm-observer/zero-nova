import { EventBus } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml } from '../utils/html';

type AgentStatus = 'pending' | 'running' | 'success' | 'failed';
type StageMode = 'parallel' | 'serial';
type PlanStatus = 'planning' | 'running' | 'reviewing' | 'completed' | 'failed';

interface AgentState {
    agentId: string;
    stageId: string;
    description: string;
    subagentType: string;
    status: AgentStatus;
    logs: string[];
    outputSummary?: string;
}

interface StageState {
    stageId: string;
    mode: StageMode;
    agents: Map<string, AgentState>;
    status: 'pending' | 'running' | 'completed' | 'failed';
}

interface PlanState {
    planId: string;
    description: string;
    stages: Map<string, StageState>;
    stageOrder: string[];
    status: PlanStatus;
    completedCount: number;
    totalCount: number;
}

type GenericPayload = Record<string, unknown> & { sessionId?: string };

export class OrchestrationView {
    private plans = new Map<string, PlanState>();

    constructor(
        private bus: EventBus,
        private container: HTMLElement,
        private getCurrentSessionId: () => string | null,
    ) {
        this.registerHandlers();
    }

    private registerHandlers() {
        this.bus.on('orchestration:plan', (payload) => this.onPlan(payload as GenericPayload));
        this.bus.on('orchestration:agent_spawn', (payload) => this.onAgentSpawn(payload as GenericPayload));
        this.bus.on('orchestration:agent_log', (payload) => this.onAgentLog(payload as GenericPayload));
        this.bus.on('orchestration:agent_complete', (payload) => this.onAgentComplete(payload as GenericPayload));
        this.bus.on('orchestration:stage_complete', (payload) => this.onStageComplete(payload as GenericPayload));
        this.bus.on('orchestration:review_start', (payload) => this.onReviewStart(payload as GenericPayload));
        this.bus.on('orchestration:complete', (payload) => this.onComplete(payload as GenericPayload));
    }

    private isCurrentSession(payload: GenericPayload): boolean {
        const sid = payload.sessionId;
        return typeof sid !== 'string' || sid === this.getCurrentSessionId();
    }

    private onPlan(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.plan_id);
        if (!planId) {
            return;
        }

        const description = this.readString(payload.description) ?? '';
        const plan: PlanState = {
            planId,
            description,
            stages: new Map(),
            stageOrder: [],
            status: 'planning',
            completedCount: 0,
            totalCount: 0,
        };

        const stages = Array.isArray(payload.stages) ? payload.stages : [];
        for (const stageSummary of stages) {
            const summary = this.readRecord(stageSummary);
            if (!summary) {
                continue;
            }
            const stageId = this.readString(summary.stage_id);
            if (!stageId) {
                continue;
            }
            const mode = this.readStageMode(summary.mode);
            const stage: StageState = {
                stageId,
                mode,
                agents: new Map(),
                status: 'pending',
            };

            const agents = Array.isArray(summary.agents) ? summary.agents : [];
            for (const agentSummary of agents) {
                const summaryAgent = this.readRecord(agentSummary);
                if (!summaryAgent) {
                    continue;
                }
                const agentId = this.readString(summaryAgent.agent_id);
                if (!agentId) {
                    continue;
                }
                stage.agents.set(agentId, {
                    agentId,
                    stageId,
                    description: this.readString(summaryAgent.description) ?? '',
                    subagentType: this.readString(summaryAgent.subagent_type) ?? 'agent',
                    status: 'pending',
                    logs: [],
                });
                plan.totalCount += 1;
            }

            plan.stages.set(stageId, stage);
            plan.stageOrder.push(stageId);
        }

        this.plans.set(plan.planId, plan);
        this.renderPlan(plan);
    }

    private onAgentSpawn(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const agentId = this.readString(payload.agent_id);
        if (!agentId) {
            return;
        }
        const agent = this.findAgent(agentId);
        if (!agent) {
            return;
        }
        agent.status = 'running';
        const stage = this.findStage(agent.stageId);
        if (stage) {
            stage.status = 'running';
        }
        const plan = this.findPlanByAgent(agentId);
        if (plan && plan.status === 'planning') {
            plan.status = 'running';
            this.updatePlanStatus(plan.planId, 'running');
        }
        this.updateAgentCard(agentId);
    }

    private onAgentLog(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const agentId = this.readString(payload.agent_id);
        const log = this.readString(payload.log);
        if (!agentId || !log) {
            return;
        }
        const agent = this.findAgent(agentId);
        if (!agent) {
            return;
        }
        agent.logs.push(log);
        this.appendAgentLog(agentId, log);
    }

    private onAgentComplete(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const agentId = this.readString(payload.agent_id);
        if (!agentId) {
            return;
        }
        const agent = this.findAgent(agentId);
        if (!agent) {
            return;
        }
        agent.status = this.readString(payload.status) === 'success' ? 'success' : 'failed';
        agent.outputSummary = this.readString(payload.output_summary);

        const plan = this.findPlanByAgent(agentId);
        if (plan) {
            plan.completedCount += 1;
            this.updatePlanProgress(plan.planId);
        }
        this.updateAgentCard(agentId);
    }

    private onStageComplete(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const stageId = this.readString(payload.stage_id);
        if (!stageId) {
            return;
        }
        const stage = this.findStage(stageId);
        if (!stage) {
            return;
        }
        stage.status = payload.all_success === true ? 'completed' : 'failed';
        this.updateStageCard(stageId, stage.status);
    }

    private onReviewStart(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.plan_id);
        if (!planId) {
            return;
        }
        const plan = this.plans.get(planId);
        if (!plan) {
            return;
        }
        plan.status = 'reviewing';
        this.updatePlanStatus(planId, 'reviewing');
    }

    private onComplete(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.plan_id);
        if (!planId) {
            return;
        }
        const plan = this.plans.get(planId);
        if (!plan) {
            return;
        }
        plan.status = payload.overall_success === true ? 'completed' : 'failed';
        plan.completedCount = plan.totalCount;
        this.updatePlanProgress(planId);
        this.updatePlanStatus(planId, plan.status);
    }

    private renderPlan(plan: PlanState) {
        const wrapper = document.createElement('div');
        wrapper.className = 'orchestration-plan';
        wrapper.id = `plan-${plan.planId}`;
        wrapper.innerHTML = `
            <div class="plan-header">
                <span class="plan-icon">⚡</span>
                <span class="plan-description">${escapeHtml(plan.description)}</span>
                <span class="plan-status-badge status-planning">规划中</span>
            </div>
            <div class="plan-progress-wrap">
                <div class="plan-progress">
                    <div class="plan-progress-bar" style="width: 0%"></div>
                </div>
                <span class="plan-progress-text">0 / ${plan.totalCount}</span>
            </div>
            <div class="plan-stages">
                ${plan.stageOrder.map((stageId) => this.renderStage(plan.stages.get(stageId))).join('')}
            </div>
            <div class="plan-review-section hidden" id="review-${plan.planId}">
                <div class="review-header">Review Agent 评审中...</div>
            </div>
        `;
        this.container.appendChild(wrapper);
        this.scrollToBottom();
    }

    private renderStage(stage?: StageState): string {
        if (!stage) {
            return '';
        }
        const modeText = stage.mode === 'parallel' ? '并行' : '串行';
        const agentsClass = stage.mode === 'parallel' ? 'agents-parallel' : 'agents-serial';
        return `
            <div class="orchestration-stage stage-${stage.mode}" id="stage-${stage.stageId}">
                <div class="stage-header">
                    <span class="stage-mode-badge">${modeText}</span>
                </div>
                <div class="stage-agents ${agentsClass}">
                    ${Array.from(stage.agents.values()).map((agent) => this.renderAgentCard(agent)).join('')}
                </div>
            </div>
        `;
    }

    private renderAgentCard(agent: AgentState): string {
        return `
            <div class="agent-card status-pending" id="agent-card-${agent.agentId}">
                <div class="agent-card-header">
                    <span class="agent-type-badge">${escapeHtml(agent.subagentType)}</span>
                    <span class="agent-description">${escapeHtml(agent.description)}</span>
                    <span class="agent-status-icon">○</span>
                </div>
                <details class="agent-log-details">
                    <summary>执行日志</summary>
                    <div class="agent-log-content" id="log-${agent.agentId}"></div>
                </details>
                <div class="agent-summary hidden" id="summary-${agent.agentId}"></div>
            </div>
        `;
    }

    private updateAgentCard(agentId: string) {
        const agent = this.findAgent(agentId);
        if (!agent) {
            return;
        }
        const card = document.getElementById(`agent-card-${agentId}`);
        if (!card) {
            return;
        }
        card.className = `agent-card status-${agent.status}`;
        const icon = card.querySelector('.agent-status-icon');
        if (icon) {
            const iconMap: Record<AgentStatus, string> = { pending: '○', running: '⟳', success: '✓', failed: '✗' };
            icon.textContent = iconMap[agent.status];
        }
        if (agent.outputSummary && (agent.status === 'success' || agent.status === 'failed')) {
            const summaryEl = document.getElementById(`summary-${agentId}`);
            if (summaryEl) {
                summaryEl.innerHTML = renderMarkdown(agent.outputSummary);
                summaryEl.classList.remove('hidden');
            }
        }
        this.scrollToBottom();
    }

    private appendAgentLog(agentId: string, log: string) {
        const logEl = document.getElementById(`log-${agentId}`);
        if (!logEl) {
            return;
        }
        const line = document.createElement('div');
        line.textContent = log;
        logEl.appendChild(line);
        logEl.scrollTop = logEl.scrollHeight;
        this.scrollToBottom();
    }

    private updateStageCard(stageId: string, status: StageState['status']) {
        const stageEl = document.getElementById(`stage-${stageId}`);
        if (!stageEl) {
            return;
        }
        stageEl.classList.remove('status-pending', 'status-running', 'status-completed', 'status-failed');
        stageEl.classList.add(`status-${status}`);
    }

    private updatePlanProgress(planId: string) {
        const plan = this.plans.get(planId);
        if (!plan) {
            return;
        }
        const planEl = document.getElementById(`plan-${planId}`);
        if (!planEl) {
            return;
        }
        const percent = plan.totalCount > 0 ? Math.round((plan.completedCount / plan.totalCount) * 100) : 0;
        const bar = planEl.querySelector<HTMLElement>('.plan-progress-bar');
        const text = planEl.querySelector<HTMLElement>('.plan-progress-text');
        if (bar) {
            bar.style.width = `${percent}%`;
        }
        if (text) {
            text.textContent = `${plan.completedCount} / ${plan.totalCount}`;
        }
    }

    private updatePlanStatus(planId: string, status: PlanStatus) {
        const planEl = document.getElementById(`plan-${planId}`);
        if (!planEl) {
            return;
        }
        const badge = planEl.querySelector<HTMLElement>('.plan-status-badge');
        if (!badge) {
            return;
        }
        const labels: Record<PlanStatus, string> = {
            planning: '规划中',
            running: '执行中',
            reviewing: '评审中',
            completed: '完成',
            failed: '失败',
        };
        badge.className = `plan-status-badge status-${status}`;
        badge.textContent = labels[status];
        if (status === 'reviewing') {
            document.getElementById(`review-${planId}`)?.classList.remove('hidden');
        }
    }

    private findAgent(agentId: string): AgentState | undefined {
        for (const plan of this.plans.values()) {
            for (const stageId of plan.stageOrder) {
                const stage = plan.stages.get(stageId);
                const agent = stage?.agents.get(agentId);
                if (agent) {
                    return agent;
                }
            }
        }
        return undefined;
    }

    private findStage(stageId: string): StageState | undefined {
        for (const plan of this.plans.values()) {
            const stage = plan.stages.get(stageId);
            if (stage) {
                return stage;
            }
        }
        return undefined;
    }

    private findPlanByAgent(agentId: string): PlanState | undefined {
        for (const plan of this.plans.values()) {
            for (const stageId of plan.stageOrder) {
                const stage = plan.stages.get(stageId);
                if (stage?.agents.has(agentId)) {
                    return plan;
                }
            }
        }
        return undefined;
    }

    private readString(input: unknown): string | undefined {
        return typeof input === 'string' && input.length > 0 ? input : undefined;
    }

    private readRecord(input: unknown): Record<string, unknown> | undefined {
        return input && typeof input === 'object' ? (input as Record<string, unknown>) : undefined;
    }

    private readStageMode(input: unknown): StageMode {
        return input === 'parallel' ? 'parallel' : 'serial';
    }

    private scrollToBottom() {
        this.container.scrollTop = this.container.scrollHeight;
    }
}
