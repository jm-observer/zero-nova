# Plan 2: Orchestrator 接管 Agent 生命周期并绑定内部 Task

## 前置依赖

Plan 1

## 任务目标

让 `OrchestratorEngine` 成为唯一的子 Agent 创建者与状态推进者。每次新增子 Agent 时，必须同步创建一个内部 task，并由 orchestrator 负责从创建到完成的全流程状态维护。

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/orchestrator/mod.rs`
  - `crates/nova-agent/src/orchestrator/scheduler.rs`
  - `crates/nova-agent/src/tool/builtin/orchestrate_task.rs`
  - `crates/nova-agent/src/tool/builtin/task.rs`
- 允许修改：
  - `crates/nova-agent/src/orchestrator/planner.rs`
  - `crates/nova-agent/src/event.rs`
  - 与 orchestrator 事件相关的测试
- 禁止修改：
  - 不要在本 Plan 删除 `TaskStore`
  - 不要实现通用任务查询产品功能
  - 不要把 task 写入职责下放回 agent 执行器

## Agent 执行步骤

1. 在 `crates/nova-agent/src/orchestrator/mod.rs` 中新增 orchestrator 侧的 task 创建与状态推进逻辑
2. 在执行每个 stage 前，为 stage 内每个 agent 创建一个内部 task；task 创建必须由 orchestrator 发起
3. 创建 task 时写入结构化编排 metadata，至少包含 `plan_id`、`stage_id`、`agent_id`、`agent_type`
4. 在 agent 真正开始执行前，将对应 task 状态更新为 `in_progress`
5. 在 agent 成功完成后，将对应 task 状态更新为 `completed`
6. 在 agent 执行失败时，必须记录失败信息；若本次不引入新失败状态，必须将错误写入 task metadata 并在文档注释中说明
7. 在 orchestrator 事件中保留 `sub_agent_spawn`、`sub_agent_complete`、`sub_agent_log`、`stage_complete` 等生命周期事件，但禁止依赖 `AgentTool` 自行发编排事件
8. `OrchestrateTaskTool` 只负责参数校验、上下文准备与调用 orchestrator，不负责创建 task

## 目标数据结构 / 接口契约

```rust
pub(crate) struct OrchestrationTaskMetadata {
    pub plan_id: String,
    pub stage_id: String,
    pub agent_id: String,
    pub agent_type: String,
}
```

| task 类型 | subject 建议格式 | owner | metadata |
|----------|------------------|-------|----------|
| orchestrator 主 task | `Orchestration: <plan description>` | `orchestrator` | `plan_id` |
| 子 agent task | `<stage_id>: <agent_id>` | `orchestrator` | `plan_id`, `stage_id`, `agent_id`, `agent_type` |

## 行为规则

| 输入 | 处理路径 | 期望输出或状态变化 |
|------|----------|------------------|
| stage 中存在 2 个并行 agent | orchestrator 先创建 2 个 task，再并发调度执行 | 两个 task 初始为 `pending`，执行时分别切到 `in_progress` |
| agent 执行成功 | orchestrator 接收执行结果 | 对应 task 变为 `completed`，并发出 `sub_agent_complete` |
| agent 执行失败 | orchestrator 接收错误 | 对应 task 保留失败信息，stage 结果记为失败 |
| stage 因依赖未满足被阻断 | orchestrator 不创建后续 agent task | 返回阻断结果，且不产生该 stage 的 agent task |

## 禁止事项

- 不要让 agent 执行器自己创建 task
- 不要恢复 `run_in_background` 一类 agent 自调度逻辑
- 不要在本 Plan 中新增“用户手工创建编排 task”的入口
- 不要把成功提示文案写入 task metadata

## 测试要求

- 修改或新增 orchestrator 测试：
  - 验证每个 agent 执行前都创建了一个 task
  - 验证 task 状态会从 `pending` 进入 `in_progress` 再到 `completed`
  - 验证失败路径会写入错误信息
  - 验证被依赖阻断的 stage 不会生成 agent task
- 必须执行验证命令：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [ ] `OrchestratorEngine` 成为唯一 agent 创建者
- [ ] 每个 agent 执行都绑定一个内部 task
- [ ] task 的创建与状态流转由 orchestrator 负责
- [ ] `AgentTool` 不再发送编排生命周期事件
- [ ] 失败路径具备可检查的 task 错误信息
- [ ] orchestrator 测试覆盖成功、失败、阻断路径
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过

