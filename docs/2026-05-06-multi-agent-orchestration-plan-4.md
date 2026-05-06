# Plan 4: 前端展示方案

- **前置依赖**：Plan 1（协议与数据模型）
- **状态**：待实施

---

## 本次目标

1. 在聊天界面中实时展示编排 Agent 树：并行组横向排列、串行组纵向堆叠
2. 每个子 Agent 以独立卡片展示：标题、状态徽章、流式日志（可折叠）
3. 整体编排进度条 + 完成/失败状态
4. Review Agent 的结果独立呈现
5. 所有展示均基于现有的 `ProgressEvent` 扩展，最小化对 `chat-view.ts` 的侵入

**可验证标准：**
- `orchestration_plan` 事件触发后，聊天区出现 Agent 树结构的占位卡片
- 并行 Stage 的子 Agent 卡片横排展示
- 每个子 Agent 的流式日志正确路由到对应卡片（`agent_id` 精确匹配）
- `sub_agent_complete` 事件后，对应卡片状态徽章更新为 ✓ 或 ✗
- `orchestration_complete` 事件后，全局进度显示 100%
- 前端不崩溃：旧版 `ProgressEvent`（无 `args`）正常渲染

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `deskapp/src/core/types.ts` | **修改** | `ProgressEvent.type` 新增编排事件 kind |
| `deskapp/src/services/chat-service.ts` | **修改** | 新增编排事件路由到 EventBus |
| `deskapp/src/ui/chat-view.ts` | **修改** | 新增 Agent 树渲染、事件监听处理 |
| `deskapp/src/ui/orchestration-view.ts` | **新增** | Agent 树 UI 组件（独立文件，降低 chat-view.ts 耦合）|
| `deskapp/src/styles/orchestration.css` | **新增** | Agent 树、卡片、进度条样式 |

---

## 详细设计

### 1. 类型扩展（`types.ts`）

```typescript
export interface ProgressEvent {
    type:
        | 'iteration' | 'thinking' | 'tool_start' | 'tool_result' | 'token'
        | 'complete' | 'turn_complete' | 'iteration_limit' | 'tool_log' | 'system_log'
        // 新增编排事件 kind：
        | 'orchestration_plan'         // 计划发布
        | 'sub_agent_spawn'            // 子 Agent 启动
        | 'sub_agent_log'              // 子 Agent 流式日志
        | 'sub_agent_complete'         // 子 Agent 完成
        | 'stage_complete'             // Stage 完成
        | 'orchestration_review_start' // Review Agent 启动
        | 'orchestration_complete';    // 整体完成

    // 现有字段不变...
    iteration?: number;
    tool?: string;
    toolUseId?: string;
    args?: Record<string, unknown>;  // 编排专用数据通过此字段传递
    result?: unknown;
    thinking?: string;
    token?: string;
    output?: string;
    description?: string;
    isError?: boolean;
    sessionId?: string;
    log?: string;
    stream?: string;
    usage?: TokenUsageView;
}
```

### 2. EventBus 路由扩展（`chat-service.ts`）

```typescript
private async handleProgress(event: any) {
    // 现有路由不变...
    if (event.type === 'token') {
        this.bus.emit('token', { sessionId: event.sessionId, token: event.token });
    } else if (event.type === 'tool_start') {
        this.bus.emit('tool:start', event);
    }
    // ... 其余现有分支 ...

    // 新增：编排事件路由
    else if (event.type === 'orchestration_plan') {
        this.bus.emit('orchestration:plan', { sessionId: event.sessionId, ...event.args });
    } else if (event.type === 'sub_agent_spawn') {
        this.bus.emit('orchestration:agent_spawn', { sessionId: event.sessionId, ...event.args });
    } else if (event.type === 'sub_agent_log') {
        this.bus.emit('orchestration:agent_log', {
            sessionId: event.sessionId,
            agentId: event.args?.agent_id,
            log: event.log,
            ...event.args
        });
    } else if (event.type === 'sub_agent_complete') {
        this.bus.emit('orchestration:agent_complete', { sessionId: event.sessionId, ...event.args });
    } else if (event.type === 'stage_complete') {
        this.bus.emit('orchestration:stage_complete', { sessionId: event.sessionId, ...event.args });
    } else if (event.type === 'orchestration_review_start') {
        this.bus.emit('orchestration:review_start', { sessionId: event.sessionId, ...event.args });
    } else if (event.type === 'orchestration_complete') {
        this.bus.emit('orchestration:complete', { sessionId: event.sessionId, ...event.args });
    }
}
```

### 3. `OrchestrationView` 组件（`orchestration-view.ts`）

```typescript
// deskapp/src/ui/orchestration-view.ts

interface AgentState {
    agentId: string;
    stageId: string;
    description: string;
    subagentType: string;
    status: 'pending' | 'running' | 'success' | 'failed';
    logs: string[];
    outputSummary?: string;
}

interface StageState {
    stageId: string;
    mode: 'parallel' | 'serial';
    agents: Map<string, AgentState>;
    status: 'pending' | 'running' | 'completed' | 'failed';
}

interface PlanState {
    planId: string;
    description: string;
    stages: Map<string, StageState>;
    stageOrder: string[];  // 拓扑序
    status: 'planning' | 'running' | 'reviewing' | 'completed' | 'failed';
    completedCount: number;
    totalCount: number;
}

export class OrchestrationView {
    private plans: Map<string, PlanState> = new Map();
    private container: HTMLElement;  // 注入 chat messages 容器

    constructor(private bus: EventBus, messagesContainer: HTMLElement) {
        this.container = messagesContainer;
        this.registerHandlers();
    }

    private registerHandlers() {
        this.bus.on('orchestration:plan', (payload) => this.onPlan(payload));
        this.bus.on('orchestration:agent_spawn', (payload) => this.onAgentSpawn(payload));
        this.bus.on('orchestration:agent_log', (payload) => this.onAgentLog(payload));
        this.bus.on('orchestration:agent_complete', (payload) => this.onAgentComplete(payload));
        this.bus.on('orchestration:stage_complete', (payload) => this.onStageComplete(payload));
        this.bus.on('orchestration:review_start', (payload) => this.onReviewStart(payload));
        this.bus.on('orchestration:complete', (payload) => this.onComplete(payload));
    }

    private onPlan(payload: any) {
        const plan: PlanState = {
            planId: payload.plan_id,
            description: payload.description,
            stages: new Map(),
            stageOrder: [],
            status: 'planning',
            completedCount: 0,
            totalCount: 0,
        };

        for (const stageSummary of payload.stages) {
            const stage: StageState = {
                stageId: stageSummary.stage_id,
                mode: stageSummary.mode,
                agents: new Map(),
                status: 'pending',
            };
            for (const agentSummary of stageSummary.agents) {
                stage.agents.set(agentSummary.agent_id, {
                    agentId: agentSummary.agent_id,
                    stageId: stageSummary.stage_id,
                    description: agentSummary.description,
                    subagentType: agentSummary.subagent_type,
                    status: 'pending',
                    logs: [],
                });
                plan.totalCount++;
            }
            plan.stages.set(stageSummary.stage_id, stage);
            plan.stageOrder.push(stageSummary.stage_id);
        }

        this.plans.set(plan.planId, plan);
        this.renderPlan(plan);
    }

    private onAgentSpawn(payload: any) {
        const agent = this.findAgent(payload.agent_id);
        if (!agent) return;
        agent.status = 'running';
        // 更新对应 Stage 状态
        const stage = this.findStage(payload.stage_id);
        if (stage) stage.status = 'running';
        this.updateAgentCard(payload.agent_id);
    }

    private onAgentLog(payload: any) {
        const agent = this.findAgent(payload.agentId);
        if (!agent || !payload.log) return;
        agent.logs.push(payload.log);
        this.appendAgentLog(payload.agentId, payload.log);
    }

    private onAgentComplete(payload: any) {
        const agent = this.findAgent(payload.agent_id);
        if (!agent) return;
        agent.status = payload.status === 'success' ? 'success' : 'failed';
        agent.outputSummary = payload.output_summary;

        // 更新进度计数
        const plan = this.findPlanByAgent(payload.agent_id);
        if (plan) plan.completedCount++;

        this.updateAgentCard(payload.agent_id);
        this.updatePlanProgress(plan?.planId);
    }

    private onStageComplete(payload: any) {
        const stage = this.findStage(payload.stage_id);
        if (!stage) return;
        stage.status = payload.all_success ? 'completed' : 'failed';
        this.updateStageCard(payload.stage_id);
    }

    private onReviewStart(payload: any) {
        const plan = this.plans.get(payload.plan_id);
        if (!plan) return;
        plan.status = 'reviewing';
        this.updatePlanStatus(payload.plan_id, 'reviewing');
    }

    private onComplete(payload: any) {
        const plan = this.plans.get(payload.plan_id);
        if (!plan) return;
        plan.status = payload.overall_success ? 'completed' : 'failed';
        this.updatePlanStatus(payload.plan_id, plan.status);
    }

    // ──────────────────────────────────────────────────────────
    // DOM 渲染
    // ──────────────────────────────────────────────────────────

    private renderPlan(plan: PlanState) {
        const el = document.createElement('div');
        el.className = 'orchestration-plan';
        el.id = `plan-${plan.planId}`;
        el.innerHTML = `
            <div class="plan-header">
                <span class="plan-icon">⚡</span>
                <span class="plan-description">${escapeHtml(plan.description)}</span>
                <span class="plan-status-badge status-planning">规划中</span>
            </div>
            <div class="plan-progress">
                <div class="plan-progress-bar" style="width: 0%"></div>
                <span class="plan-progress-text">0 / ${plan.totalCount}</span>
            </div>
            <div class="plan-stages">
                ${plan.stageOrder.map(sid => this.renderStage(plan.stages.get(sid)!)).join('')}
            </div>
            <div class="plan-review-section hidden" id="review-${plan.planId}">
                <div class="review-header">
                    <span class="review-icon">🔍</span> Review Agent 评审中...
                </div>
            </div>
        `;
        this.container.appendChild(el);
        this.scrollToBottom();
    }

    private renderStage(stage: StageState): string {
        return `
            <div class="orchestration-stage stage-${stage.mode}" id="stage-${stage.stageId}">
                <div class="stage-header">
                    <span class="stage-mode-badge">${stage.mode === 'parallel' ? '⚡ 并行' : '→ 串行'}</span>
                </div>
                <div class="stage-agents ${stage.mode === 'parallel' ? 'agents-parallel' : 'agents-serial'}">
                    ${Array.from(stage.agents.values()).map(a => this.renderAgentCard(a)).join('')}
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
        if (!agent) return;
        const card = document.getElementById(`agent-card-${agentId}`);
        if (!card) return;

        // 更新状态 class 和图标
        card.className = `agent-card status-${agent.status}`;
        const icon = card.querySelector('.agent-status-icon');
        if (icon) {
            icon.textContent = {
                pending: '○',
                running: '⟳',
                success: '✓',
                failed: '✗',
            }[agent.status];
        }

        // 展示 output_summary
        if (agent.outputSummary && (agent.status === 'success' || agent.status === 'failed')) {
            const summaryEl = document.getElementById(`summary-${agentId}`);
            if (summaryEl) {
                summaryEl.innerHTML = renderMarkdown(agent.outputSummary);
                summaryEl.classList.remove('hidden');
            }
        }
    }

    private appendAgentLog(agentId: string, log: string) {
        const logEl = document.getElementById(`log-${agentId}`);
        if (!logEl) return;
        const span = document.createElement('span');
        span.textContent = log;
        logEl.appendChild(span);
        // 自动滚动日志区域
        logEl.scrollTop = logEl.scrollHeight;
    }

    private updatePlanProgress(planId: string | undefined) {
        if (!planId) return;
        const plan = this.plans.get(planId);
        if (!plan) return;
        const pct = plan.totalCount > 0
            ? Math.round((plan.completedCount / plan.totalCount) * 100)
            : 0;
        const planEl = document.getElementById(`plan-${planId}`);
        if (!planEl) return;
        const bar = planEl.querySelector<HTMLElement>('.plan-progress-bar');
        const text = planEl.querySelector<HTMLElement>('.plan-progress-text');
        if (bar) bar.style.width = `${pct}%`;
        if (text) text.textContent = `${plan.completedCount} / ${plan.totalCount}`;
    }

    private updatePlanStatus(planId: string, status: PlanState['status']) {
        const planEl = document.getElementById(`plan-${planId}`);
        if (!planEl) return;
        const badge = planEl.querySelector<HTMLElement>('.plan-status-badge');
        if (!badge) return;

        const labels: Record<string, string> = {
            planning: '规划中',
            running: '执行中',
            reviewing: '评审中',
            completed: '完成 ✓',
            failed: '失败 ✗',
        };
        badge.textContent = labels[status] ?? status;
        badge.className = `plan-status-badge status-${status}`;

        if (status === 'reviewing') {
            document.getElementById(`review-${planId}`)?.classList.remove('hidden');
        }
    }
}
```

### 4. CSS 样式（`orchestration.css`）

```css
/* 整体计划块 */
.orchestration-plan {
    border: 1px solid var(--border-color);
    border-radius: 10px;
    margin: 12px 0;
    overflow: hidden;
    background: var(--bg-secondary);
}

.plan-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border-color);
    font-weight: 500;
}

/* 进度条 */
.plan-progress {
    height: 4px;
    background: var(--border-color);
    position: relative;
}
.plan-progress-bar {
    height: 100%;
    background: var(--accent-color);
    transition: width 0.3s ease;
}
.plan-progress-text {
    position: absolute;
    right: 8px;
    top: 6px;
    font-size: 0.75em;
    color: var(--text-secondary);
}

/* Stage */
.orchestration-stage {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-color);
}
.orchestration-stage:last-child { border-bottom: none; }

.stage-header {
    margin-bottom: 8px;
    font-size: 0.8em;
    color: var(--text-secondary);
}

/* 并行：横排 */
.agents-parallel {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 8px;
}

/* 串行：纵向堆叠 */
.agents-serial {
    display: flex;
    flex-direction: column;
    gap: 6px;
}

/* Agent 卡片 */
.agent-card {
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 8px 12px;
    background: var(--bg-primary);
    transition: border-color 0.2s, box-shadow 0.2s;
}
.agent-card.status-running {
    border-color: var(--accent-color);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-color) 20%, transparent);
}
.agent-card.status-success { border-color: #22c55e; }
.agent-card.status-failed  { border-color: #ef4444; }

.agent-card-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.9em;
}
.agent-type-badge {
    font-size: 0.75em;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
}
.agent-description { flex: 1; }
.agent-status-icon {
    font-size: 1em;
    width: 16px;
    text-align: center;
}
.status-running .agent-status-icon { animation: spin 1s linear infinite; }

/* 日志区 */
.agent-log-details summary {
    font-size: 0.78em;
    color: var(--text-secondary);
    cursor: pointer;
    margin-top: 6px;
    user-select: none;
}
.agent-log-content {
    font-family: var(--font-code);
    font-size: 0.75em;
    max-height: 160px;
    overflow-y: auto;
    padding: 6px;
    background: #0d1117;
    color: #e6edf3;
    border-radius: 4px;
    margin-top: 4px;
    white-space: pre-wrap;
    word-break: break-all;
}

/* 摘要 */
.agent-summary {
    margin-top: 6px;
    padding: 6px;
    background: var(--bg-secondary);
    border-radius: 4px;
    font-size: 0.85em;
}

/* 徽章状态色 */
.plan-status-badge {
    font-size: 0.75em;
    padding: 2px 8px;
    border-radius: 10px;
    background: var(--bg-secondary);
}
.status-running  { background: color-mix(in srgb, var(--accent-color) 15%, transparent); color: var(--accent-color); }
.status-completed { background: #dcfce7; color: #16a34a; }
.status-failed   { background: #fee2e2; color: #dc2626; }
.status-reviewing { background: #fef3c7; color: #d97706; }

/* Review 区块 */
.plan-review-section {
    padding: 10px 14px;
    border-top: 1px solid var(--border-color);
    font-size: 0.9em;
}
.review-header {
    font-weight: 500;
    margin-bottom: 6px;
    color: var(--text-secondary);
}

/* 旋转动画 */
@keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
}
```

### 5. 与 `chat-view.ts` 的整合点

在 `ChatView` 的构造函数中，实例化 `OrchestrationView` 并传入消息容器：

```typescript
// chat-view.ts 构造函数末尾
import { OrchestrationView } from './orchestration-view';

// 在 constructor 中：
this.orchestrationView = new OrchestrationView(bus, this.messagesContainer);
```

这样做的优点：
- `chat-view.ts` 只增加 2 行，不污染现有逻辑
- `OrchestrationView` 独立管理 Agent 树的全部 DOM 和状态
- 编排 UI 渲染与普通消息渲染完全解耦

---

## 视觉效果示意

```
╔══════════════════════════════════════════════════════════════╗
║ ⚡ 实现用户认证系统                       [执行中]           ║
║ ████████████░░░░░░░░  2 / 3                                  ║
╠══════════════════════════════════════════════════════════════╣
║  ⚡ 并行 Stage 1                                             ║
║  ┌─────────────────────┐  ┌─────────────────────┐           ║
║  │ [Coder]  数据库模型 ✓│  │ [Coder]  API 路由  ✓│           ║
║  │ ▼ 执行日志           │  │ ▼ 执行日志           │           ║
║  │  读取 schema...      │  │  创建路由文件...      │           ║
║  │──────────────────── │  │──────────────────── │           ║
║  │ 实现了 User、Token   │  │ 实现了 /login 和     │           ║
║  │ 两个数据库模型        │  │ /register 端点       │           ║
║  └─────────────────────┘  └─────────────────────┘           ║
╠══════════════════════════════════════════════════════════════╣
║  → 串行 Stage 2（依赖 Stage 1）                              ║
║  ┌─────────────────────────────────────────────────────┐    ║
║  │ [Coder]  集成测试                            ⟳       │    ║
║  │ ▼ 执行日志                                           │    ║
║  │  正在编写 auth_test.rs...                            │    ║
║  └─────────────────────────────────────────────────────┘    ║
╠══════════════════════════════════════════════════════════════╣
║ 🔍 Review Agent 评审中...                                    ║
║  [Review Agent 的流式输出正常显示在此处]                      ║
╚══════════════════════════════════════════════════════════════╝
```

---

## 测试案例

### T4-01：计划渲染完整性
- **输入**：发送 `orchestration:plan` 事件，含 2 个 Stage（parallel + serial）
- **预期**：消息区出现 Plan 块，Stage 1 横排 2 个卡片，Stage 2 纵向 1 个卡片

### T4-02：并行卡片横排布局
- **输入**：Stage mode=parallel，含 3 个 Agent
- **预期**：3 个卡片使用 CSS grid 横向排列，宽度 ≤ 容器宽度（无横向滚动条）

### T4-03：日志路由精确性
- **输入**：发送 `orchestration:agent_log`，`agent_id = "a2"`
- **预期**：日志内容只出现在 `a2` 的卡片日志区，`a1` 卡片无变化

### T4-04：状态徽章更新
- **输入**：`sub_agent_complete`，`agent_id = "a1"`，`status = "success"`
- **预期**：`a1` 卡片边框变绿，状态图标变 ✓，output_summary 文本显示

### T4-05：失败状态视觉
- **输入**：`sub_agent_complete`，`status = "failed"`，`error = "compile error"`
- **预期**：卡片边框变红，状态图标变 ✗，无 output_summary 显示

### T4-06：进度条精度
- **输入**：Plan 共 4 个 Agent，依次完成 2 个
- **预期**：进度条显示 50%，文字 "2 / 4"

### T4-07：Review Agent 区块出现时机
- **输入**：发送 `orchestration:review_start`
- **预期**：Plan 底部 Review 区块从 hidden 变为可见，状态徽章变"评审中"

### T4-08：完成状态
- **输入**：`orchestration:complete`，`overall_success = true`
- **预期**：进度条满格，状态徽章变"完成 ✓"（绿色），Review 区块显示 Review Agent 输出

### T4-09：旧版事件兼容性
- **输入**：发出 `tool_start`、`tool_log` 等旧版事件（无 `args.agent_id`）
- **预期**：旧版工具卡片正常渲染，`OrchestrationView` 不响应，无 JS 错误

### T4-10：窗口缩放响应
- **输入**：将聊天窗口宽度缩小至 400px
- **预期**：并行卡片自动改为单列（CSS grid `auto-fit minmax`），不出现横向溢出

### T4-11：多 Plan 并发（保留性测试）
- **输入**：同一 session 连续发送 2 个 `orchestration_plan` 事件（不同 plan_id）
- **预期**：两个 Plan 块独立显示，各自的事件互不干扰（通过 plan_id / agent_id 路由）
