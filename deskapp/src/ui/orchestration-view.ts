import { EventBus } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml } from '../utils/html';

type AgentStatus = 'pending' | 'running' | 'success' | 'failed' | 'cancelled';
type StageMode = 'parallel' | 'serial';
type PlanStatus = 'planning' | 'running' | 'reviewing' | 'completed' | 'failed';

interface AgentState {
    planId: string;
    agentId: string;
    stageId: string;
    description: string;
    subagentType: string;
    status: AgentStatus;
    logs: string[];
    outputSummary?: string;
}

interface StageState {
    planId: string;
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
    agentIndex: Map<string, AgentState>;
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
        const planId = this.readString(payload.planId);
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
            agentIndex: new Map(),
        };

        const stages = Array.isArray(payload.stages) ? payload.stages : [];
        for (const stageSummary of stages) {
            const summary = this.readRecord(stageSummary);
            if (!summary) {
                continue;
            }
            const stageId = this.readString(summary.stageId);
            if (!stageId) {
                continue;
            }
            const mode = this.readStageMode(summary.mode);
            const stage: StageState = {
                planId,
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
                const agentId = this.readString(summaryAgent.agentId);
                if (!agentId) {
                    continue;
                }
                const agentState: AgentState = {
                    planId,
                    agentId,
                    stageId,
                    description: this.readString(summaryAgent.description) ?? '',
                    subagentType: this.readString(summaryAgent.subagentType) ?? 'agent',
                    status: 'pending',
                    logs: [],
                };
                stage.agents.set(agentId, agentState);
                plan.agentIndex.set(agentId, agentState);
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
        const planId = this.readString(payload.planId);
        const agentId = this.readString(payload.agentId);
        if (!planId || !agentId) {
            return;
        }
        const agent = this.findAgent(planId, agentId);
        if (!agent) {
            return;
        }
        agent.status = 'running';
        const stage = this.findStage(planId, agent.stageId);
        if (stage) {
            stage.status = 'running';
            this.updateStageCard(planId, stage.stageId, stage.status);
        }
        const plan = this.plans.get(planId);
        if (plan && plan.status === 'planning') {
            plan.status = 'running';
            this.updatePlanStatus(plan.planId, 'running');
        }
        this.updateAgentCard(planId, agentId);
    }

    private onAgentLog(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.planId);
        const agentId = this.readString(payload.agentId);
        const log = this.readString(payload.log);
        if (!planId || !agentId || !log) {
            return;
        }
        const agent = this.findAgent(planId, agentId);
        if (!agent) {
            return;
        }
        agent.logs.push(log);
        this.appendAgentLog(planId, agentId, log);
    }

    private onAgentComplete(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.planId);
        const agentId = this.readString(payload.agentId);
        if (!planId || !agentId) {
            return;
        }
        const agent = this.findAgent(planId, agentId);
        if (!agent) {
            return;
        }
        agent.status = this.readAgentStatus(payload.status);
        agent.outputSummary = this.readString(payload.outputSummary) ?? this.readString(payload.error);

        const plan = this.plans.get(planId);
        if (plan) {
            plan.completedCount += 1;
            this.updatePlanProgress(plan.planId);
        }
        this.updateAgentCard(planId, agentId);
    }

    private onStageComplete(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.planId);
        const stageId = this.readString(payload.stageId);
        if (!planId || !stageId) {
            return;
        }
        const stage = this.findStage(planId, stageId);
        if (!stage) {
            return;
        }
        stage.status = payload.allSuccess === true ? 'completed' : 'failed';
        this.updateStageCard(planId, stageId, stage.status);
    }

    private onReviewStart(payload: GenericPayload) {
        if (!this.isCurrentSession(payload)) {
            return;
        }
        const planId = this.readString(payload.planId);
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
        const planId = this.readString(payload.planId);
        if (!planId) {
            return;
        }
        const plan = this.plans.get(planId);
        if (!plan) {
            return;
        }
        plan.status = payload.overallSuccess === true ? 'completed' : 'failed';
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
            <div class="orchestration-stage stage-${stage.mode}" id="${this.stageDomId(stage.planId, stage.stageId)}">
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
            <div class="agent-card status-pending" id="${this.agentDomId(agent.planId, agent.agentId)}">
                <div class="agent-card-header">
                    <span class="agent-type-badge">${escapeHtml(agent.subagentType)}</span>
                    <span class="agent-description">${escapeHtml(agent.description)}</span>
                    <span class="agent-status-icon">○</span>
                </div>
                <details class="agent-log-details">
                    <summary>执行日志</summary>
                    <div class="agent-log-content" id="${this.logDomId(agent.planId, agent.agentId)}"></div>
                </details>
                <div class="agent-summary hidden" id="${this.summaryDomId(agent.planId, agent.agentId)}"></div>
            </div>
        `;
    }

    private updateAgentCard(planId: string, agentId: string) {
        const agent = this.findAgent(planId, agentId);
        if (!agent) {
            return;
        }
        const card = document.getElementById(this.agentDomId(planId, agentId));
        if (!card) {
            return;
        }
        card.className = `agent-card status-${agent.status}`;
        const icon = card.querySelector('.agent-status-icon');
        if (icon) {
            const iconMap: Record<AgentStatus, string> = {
                pending: '○',
                running: '⟳',
                success: '✓',
                failed: '✗',
                cancelled: '⊘',
            };
            icon.textContent = iconMap[agent.status];
        }
        if (agent.outputSummary && agent.status !== 'running' && agent.status !== 'pending') {
            const summaryEl = document.getElementById(this.summaryDomId(planId, agentId));
            if (summaryEl) {
                summaryEl.innerHTML = renderMarkdown(agent.outputSummary);
                summaryEl.classList.remove('hidden');
            }
        }
        this.scrollToBottom();
    }

    private appendAgentLog(planId: string, agentId: string, log: string) {
        const logEl = document.getElementById(this.logDomId(planId, agentId));
        if (!logEl) {
            return;
        }
        const line = document.createElement('div');
        line.textContent = log;
        logEl.appendChild(line);
        logEl.scrollTop = logEl.scrollHeight;
        this.scrollToBottom();
    }

    private updateStageCard(planId: string, stageId: string, status: StageState['status']) {
        const stageEl = document.getElementById(this.stageDomId(planId, stageId));
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

    private findAgent(planId: string, agentId: string): AgentState | undefined {
        return this.plans.get(planId)?.agentIndex.get(agentId);
    }

    private findStage(planId: string, stageId: string): StageState | undefined {
        return this.plans.get(planId)?.stages.get(stageId);
    }

    private readAgentStatus(input: unknown): AgentStatus {
        if (input === 'success' || input === 'failed' || input === 'cancelled' || input === 'running') {
            return input;
        }
        return 'failed';
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

    private agentDomId(planId: string, agentId: string): string {
        return `agent-card-${planId}-${agentId}`;
    }

    private logDomId(planId: string, agentId: string): string {
        return `log-${planId}-${agentId}`;
    }

    private summaryDomId(planId: string, agentId: string): string {
        return `summary-${planId}-${agentId}`;
    }

    private stageDomId(planId: string, stageId: string): string {
        return `stage-${planId}-${stageId}`;
    }

    private scrollToBottom() {
        this.container.scrollTop = this.container.scrollHeight;
    }
}

