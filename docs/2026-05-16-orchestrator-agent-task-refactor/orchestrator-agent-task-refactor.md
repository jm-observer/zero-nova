# Orchestrator 接管 Agent 与 Task 状态设计

## 时间

- 创建时间：2026-05-16
- 最后更新：2026-05-16

## 项目现状

当前多 Agent 编排链路由 `OrchestrateTaskTool`、`AgentTool`、`Task*Tool` 共同组成，但职责边界不清：

1. `AgentTool` 同时承担模型可见工具、子 Agent 执行器、prompt 构建入口三种职责
2. `AgentTool` 输入同时混合执行参数、编排参数、展示参数，字段数量过多（12 个字段）且存在重复语义
3. `OrchestratorEngine` 负责 stage 调度，但不负责创建和维护 task，只通过事件流观察 agent 生命周期
4. `OrchestrateTaskTool` 每次调用都重新构造一个 `OrchestratorEngine`（无状态、一次性），与"orchestrator 负责 agent 生命周期"的目标存在张力
5. `TaskStore` 目前通过 `TaskCreate` / `TaskUpdate` / `TaskList` 暴露为通用任务工具，但编排任务仅通过弱类型 metadata 约定挂接
6. `stage_id`、`agent_id` 已在编排模型中存在，但仍以工具输入字段的形式泄露到 `AgentTool` 协议中
7. `OrchestrateTask` 的 `planJson` 参数为 JSON 字符串类型（JSON-in-JSON），模型需要在 JSON 内生成转义后的 JSON，出错概率高；且对单 Agent 场景参数负担过重——需要填写 `planId`、`stageId`、`agentId`、`mode`、`dependsOn` 等纯冗余字段

这些问题导致：

- 模型层可以直接操作本应只属于编排层的 agent 生命周期
- orchestrator 与 agent 执行器之间通过 JSON 工具协议耦合，内部调用链路过长
- task 既像产品功能，又像内部状态表，后续很难稳定演进
- 移除 `AgentTool` 后，单 Agent 场景被迫走完整编排 plan，体验退化严重

## 整体目标

本次重构目标是将多 Agent 编排能力收敛为“编排器主导，执行器内聚，任务内部化”模型：

1. `AgentTool` 取消 `Tool` 属性，不再作为模型可见工具注册
2. `AgentTool` 收口为内部 `AgentExecutor` / `SubAgentExecutor`，只接受最小执行参数
3. `OrchestratorEngine` 成为唯一的子 Agent 创建者，负责分配 `stage_id`、`agent_id` 并驱动执行
4. 每次 orchestrator 新增子 Agent 时，必须同步创建一个内部 task，并由 orchestrator 维护其状态流转
5. `TaskStore` 保留通用任务管理能力（`TaskCreate` / `TaskUpdate` / `TaskList` 工具保留），但编排产生的 task 由 orchestrator 内部写入，不再经由模型直接操作
6. `OrchestrateTaskTool` 保持为唯一对外的编排入口
7. `OrchestrateTask` 支持快捷模式：单 Agent 场景可省略 plan 结构，仅传入 `prompt` + `description`；内部统一构造为单阶段计划执行
8. `OrchestrateTask` 的完整模式将 `planJson`（字符串）改为 `plan`（object），消除 JSON-in-JSON

重构后的核心原则：

- 编排入口唯一
- Agent 生命周期归 orchestrator
- task 分为两类：通用任务（模型可操作）与编排任务（orchestrator 独占）
- 模型不再直接感知 `stage_id` / `agent_id`

## Plan 拆分

| Plan | 描述 | 依赖 | 顺序 | 状态 |
|------|------|------|------|------|
| Plan 1 | 收缩 `AgentTool` 为内部执行器，移除模型工具入口与过宽参数协议；同步为 `OrchestrateTask` 增加快捷模式与 plan object 化 | 无 | 1 | 待开始 |
| Plan 2 | 让 `OrchestratorEngine` 成为唯一 agent 创建者，并将 agent 生命周期绑定到内部 task | Plan 1 | 2 | 待开始 |
| Plan 3 | 将编排 task 写入路径收归 orchestrator 内部，保留通用 `Task*Tool`，补齐测试与长期设计资产 | Plan 1, Plan 2 | 3 | 待开始 |

### Plan 关键动作点

**Plan 1：**
- 决定是新建 `AgentExecutor` struct 还是原地改造 `AgentTool`（移除 `Tool` impl）
- 从 input schema 中移除的字段清单：`agent_id`、`parent_plan_id`、`stage_id`、`skill_id`、`injection_mode`、`output_format`、`run_in_background`（这些由编排层内部传递）
- `SubAgentExecutor` trait 的方法签名从 `serde_json::Value` 改为强类型参数
- 为 `OrchestrateTask` 增加快捷模式 input schema（`prompt` + `description` + 可选 `agentSelection`）
- 将 `planJson: string` 改为 `plan: object`，消除 JSON-in-JSON
- 排查 system prompt 和 skill 定义中对 `Agent` 工具名的引用，确保移除后不会运行时找不到工具

**Plan 2：**
- 明确 `OrchestratorEngine` 是保持每次调用新建实例，还是改为可持续持有；若保持一次性，则 task 创建在 `execute_plan` 流程内完成
- orchestrator 在创建子 Agent 时同步创建编排 task，在 agent 完成/失败/取消时同步更新 task 状态
- 取消（`CancellationToken`）触发时需同步将关联 task 状态更新为 `Cancelled`（需确认是否新增该状态值）

**Plan 3：**
- `TaskCreate` / `TaskUpdate` / `TaskList` 保留为通用任务管理工具
- 编排产生的 task 由 orchestrator 内部通过 `TaskStore` 写入，不经由工具协议
- 编排 task 与通用 task 通过 `metadata` 中的 `orchestration_*` 键或专用字段区分
- 补齐测试覆盖，更新 `docs/design/system-overview.md` 和 `docs/design/nova-agent-engine-boundaries.md`

## 风险与待定项

### 原有风险

- `TaskStore` 若完全只保留内存态，后续恢复 / 重试能力仍有限；本次先不引入持久化
- `TaskStatus` 当前只有 `pending` / `in_progress` / `completed` / `deleted`，是否需要显式失败/取消态（`Failed` / `Cancelled`），本次需在实施时定稿
- `stage_id` 是否继续来自 plan 输入，还是由 orchestrator 在解析后做规范化生成，本次默认保留 plan 中的 stage 标识
- 长期设计资产预计至少需要更新：
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`

### 新增风险（review 补充）

- **向后兼容 — prompt 与 skill 引用**：移除 `Agent` 工具后，现有 system prompt 模板和 skill 定义中可能引用了 `Agent` 工具名，需排查确保运行时不会找不到工具
- **事件协议变更**：前端（`deskapp`）消费 `orchestration_progress` 中的 `sub_agent_spawn` / `sub_agent_complete` 等事件；如果 orchestrator 事件发射逻辑变更（如 task 状态事件替代 agent 状态事件），需同步更新前端事件处理
- **`SubAgentExecutor` trait 归属**：当前定义在 `orchestrator/mod.rs`，重构后若执行器不再是 Tool，trait 方法签名（接收 `serde_json::Value`）是否改为强类型参数需在 Plan 1 中决定
- **`planJson` → `plan` 的协议迁移**：将字符串改为 object 是 breaking change，需确认是否需要过渡期同时支持两种格式

### 设计约束

- `OrchestrationPlan` / `ExecutionStage` / `AgentRequest` 等 planner 数据结构保持不变；orchestrator 与 task 的绑定关系在 orchestrator 侧维护独立 mapping，不侵入 plan 结构
- 取消（`CancellationToken`）触发时，orchestrator 必须同步更新关联 task 状态

