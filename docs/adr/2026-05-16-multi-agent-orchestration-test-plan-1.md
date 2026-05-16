# 2026-05-16 Multi-Agent Orchestration Plan 1

## 背景

`OrchestratorEngine` 原先直接持有 `Arc<AgentTool>`。这让 orchestration 核心流程测试必须经过真实 `AgentTool`，难以隔离 plan 执行、review 和事件发射逻辑，也阻碍了后续为 engine 编写纯单元测试。

## 决策

为 `OrchestratorEngine` 引入 `SubAgentExecutor` trait，抽象出三项稳定依赖：

- 执行子代理
- 获取 catalog agent id 集合
- 获取默认 agent id

生产环境继续由 `AgentTool` 实现该 trait，`OrchestrateTaskTool` 仍负责构造 engine 并注入真实执行器。

## 影响范围

- `crates/nova-agent/src/orchestrator/mod.rs`
- `crates/nova-agent/src/tool/builtin/agent.rs`
- `crates/nova-agent/src/tool/builtin/orchestrate_task.rs`
- orchestration 相关测试编写方式

## 取舍

- 放弃继续直接依赖 `AgentTool`：这样虽然改动更小，但无法在 engine 层注入 mock。
- 放弃把更多 orchestration 依赖一并抽象成复杂 service 层：当前只抽取测试所需最小边界，避免过度设计。

## 文档同步

- 更新 `docs/design/nova-agent-engine-boundaries.md`
- 更新 `docs/design/system-overview.md`

## 关联项

- `docs/2026-05-16-multi-agent-orchestration-test.md`
- `docs/2026-05-16-multi-agent-orchestration-test-plan-1.md`
