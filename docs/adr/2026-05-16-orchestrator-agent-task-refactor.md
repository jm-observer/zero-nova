# 2026-05-16 Orchestrator 接管 Agent 与 Task 状态

## 背景

当前多 Agent 编排链路中：

- `AgentTool` 同时是模型工具和内部执行器
- orchestrator 通过工具 JSON 协议间接调用子 Agent
- `TaskStore` 同时承担通用任务系统和编排状态表职责

这导致模型层可以直接接触编排内部协议，编排生命周期与任务状态也无法由单一组件负责。

## 决策

采用路线 A：**保留 task，但仅内部使用**，并将多 Agent 编排能力收敛为以下模型：

- `OrchestrateTaskTool` 作为唯一对外编排入口
- `OrchestratorEngine` 作为唯一子 Agent 创建者与 task 状态推进者
- `AgentTool` 取消 `Tool` 属性，退回为内部 `SubAgentExecutor`
- `TaskStore` 保留，但仅作为 orchestrator 内部状态模型，不再允许模型直接写入

进一步约束：

- `stage_id` 表示 plan 内的执行阶段标识
- `agent_id` 表示某个 stage 内的具体子 Agent 实例标识
- `stage_id`、`agent_id` 只在 orchestrator 与内部执行器之间传递，不再暴露为模型可调用工具参数

## 影响范围

- `crates/nova-agent/src/tool/builtin/agent.rs`
- `crates/nova-agent/src/tool/builtin/orchestrate_task.rs`
- `crates/nova-agent/src/tool/builtin/task.rs`
- `crates/nova-agent/src/tool/builtin/mod.rs`
- `crates/nova-agent/src/orchestrator/mod.rs`
- `crates/nova-agent/src/orchestrator/scheduler.rs`
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- 前端 / app 层对 session task 状态的读取逻辑

## 取舍

放弃方案：

- 完全删除 task
  - 原因：会失去结构化编排状态、前端任务视图、失败定位与后续恢复扩展点
- 保留 `AgentTool` 作为模型工具，只减少字段
  - 原因：职责边界仍然混乱，模型仍能绕过 orchestrator 直接触达内部生命周期

保留方案：

- 保留 `TaskStore`
  - 原因：适合作为 orchestrator 的内部状态表与观察面
- 保留 `stage_id` / `agent_id`
  - 原因：便于区分 stage 边界与并行子 Agent 实例

## 文档同步

需要更新：

- `docs/design/system-overview.md`
- `docs/design/nova-agent-engine-boundaries.md`

本次任务级文档：

- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-1.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-2.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-3.md`

## 关联项

- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-1.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-2.md`
- `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor-plan-3.md`
