# Multi-Agent Orchestration 测试设计

- **创建时间**：2026-05-16
- **状态**：设计中

## 项目现状

`nova-agent` 的 orchestrator 子系统由 4 个核心模块组成：

| 模块 | 文件 | 现有测试数 | 覆盖现状 |
|------|------|-----------|---------|
| OrchestratorEngine | `orchestrator/mod.rs` | 0 | 完全无测试 |
| Planner | `orchestrator/planner.rs` | 3 | 仅错误路径（unknown dep, cycle, default type） |
| Scheduler | `orchestrator/scheduler.rs` | 3 | 仅失败/取消路径，无 happy path |
| Reviewer | `orchestrator/reviewer.rs` | 1 | 仅失败详情 prompt |

总计 16 个 orchestration 相关测试（含 `agent.rs` 7 个 + `orchestrate_task.rs` 2 个），核心协调逻辑几乎未被覆盖。

## 整体目标

1. 为 `OrchestratorEngine` 引入可 mock 的 trait 抽象，使其核心逻辑可测试
2. 补全所有模块的 happy path 和关键边界场景
3. 所有测试通过 `cargo test` 运行，不依赖 LLM，CI 可跑
4. 新增约 40 个测试用例（Plan 2: 18 个 + Plan 3: 23 个）

## Plan 拆分

| Plan | 标题 | 依赖 | 说明 | 状态 |
|------|------|------|------|------|
| Plan 1 | SubAgentExecutor trait 重构 | 无 | 抽取 trait，改造 Engine 构造函数 | 已完成 |
| Plan 2 | OrchestratorEngine 测试 | Plan 1 | Engine 层全部测试用例 | 已完成 |
| Plan 3 | Scheduler / Planner / Reviewer 补全 | 无（与 Plan 1/2 可并行） | 各模块现有测试基础上补全 | 待开始 |

## 风险与待定项

- trait 重构需确保 `OrchestrateTaskTool` 等现有调用点不受影响
- `rewire_log_forwarding` 的测试需要构造 `ToolContext`，可能需要 test helper
