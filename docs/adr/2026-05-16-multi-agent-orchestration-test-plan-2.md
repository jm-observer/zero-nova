# 2026-05-16 Multi-Agent Orchestration Plan 2

## 背景

`OrchestratorEngine` 在 `Plan 1` 完成后已经可以注入 mock executor，但核心执行路径仍缺少系统化单元测试，尤其是 stage 失败、依赖阻塞、取消前 review、事件发射和日志转发等场景。

## 决策

在 `crates/nova-agent/src/orchestrator/mod.rs` 中补充完整测试模块，并将失败路径行为固定为：

- 任一 stage 失败后，不进入 review
- 如果后续 stage 依赖失败 stage，返回 dependency blocked 摘要
- 如果失败 stage 后没有可执行的后续 stage，直接返回 orchestration stopped 摘要

## 影响范围

- `crates/nova-agent/src/orchestrator/mod.rs`
- `docs/2026-05-16-multi-agent-orchestration-test.md`
- `docs/design/nova-agent-engine-boundaries.md`

## 取舍

- 放弃只补测试、不修正失败路径：那样测试无法稳定通过，且行为与测试设计不一致。
- 放弃继续抽出额外 test helper 文件：当前测试基础设施只在 `mod.rs` 内部使用，保持局部即可。

## 文档同步

- 更新 `docs/2026-05-16-multi-agent-orchestration-test.md`
- 更新 `docs/design/nova-agent-engine-boundaries.md`

## 关联项

- `docs/2026-05-16-multi-agent-orchestration-test-plan-2.md`
- `docs/2026-05-16-multi-agent-orchestration-test.md`
