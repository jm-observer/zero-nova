# Plan 5: 渐进实施策略、验证矩阵与回滚预案

## 前置依赖
- Plan 2
- Plan 3
- Plan 4

## 本次目标
- 给出可执行的实施节奏，避免一次性大改带来的风险叠加。
- 建立拆分任务的验证矩阵（编译、静态检查、测试、行为抽样）。
- 形成可落地回滚预案与提交策略，支持快速止损。

## 涉及文件
- `docs/2026-05-10-nova-agent-bigfile-split/nova-agent-bigfile-split.md`
- `docs/2026-05-10-nova-agent-bigfile-split/nova-agent-bigfile-split-plan-5.md`
- `crates/nova-agent/src/**`（实施阶段）
- `crates/nova-agent/tests/**`（补充回归用例）

## 详细设计
1. 实施批次
- 批次 A：`prompt` + `config`（低耦合高收益）。
- 批次 B：`conversation` + `app`（服务/存储边界重构）。
- 批次 C：`agent` + `skill` + `tool`（运行时核心收敛）。
- 每个批次拆为 2-4 个小 PR，单 PR 聚焦单一职责。

2. 验证矩阵
- 编译检查：`cargo check --workspace`。
- 质量门禁：`cargo clippy --workspace -- -D warnings`。
- 格式检查：`cargo fmt --all --check`。
- 行为回归：`cargo test --workspace` + 关键集成测试点抽样。
- 结构目标：新增/调整后文件行数统计，确认超限文件显著下降。

3. 提交与回滚策略
- 提交粒度：一个提交只包含同一职责模块的搬迁和最小联动。
- 回滚手段：按批次/PR 可独立 revert，不影响未开始批次。
- 风险控制：若某批次修复流程失败超过 2 轮，暂停后缩小变更面并拆更小子任务。

4. 完成标准
- 高优先超大文件全部进入拆分路径并完成迁移。
- 修复流程在每个批次末尾全部通过。
- 总览文档 Plan 状态更新为「已完成」，并记录最终文件行数对比。

## 测试案例
- 正常路径：各批次完成后全量门禁通过。
- 边界条件：仅执行某一批次回滚后，仓库仍可编译并通过核心测试。
- 异常场景：子模块导出遗漏导致编译失败，可在 CI 第一时间暴露并定位。

## 实施结果（2026-05-10）
1. 批次落地情况
- 批次 A（`prompt` + `config`）：已按子模块方式落地，外部导出路径保持稳定。
- 批次 B（`conversation` + `app`）：已完成服务/仓储边界拆分，调用链保持不变。
- 批次 C（`agent` + `skill` + `tool`）：已完成运行时核心、技能注册与工具注册域拆分。

2. 验证矩阵执行记录
- 编译检查：纳入修复流程由 `cargo test --workspace` 间接覆盖编译通过。
- 质量门禁：`cargo clippy --workspace -- -D warnings`（通过）。
- 格式检查：`cargo fmt --all --check`（通过）。
- 行为回归：`cargo test --workspace`（通过）。
- 结构目标：高优先拆分路径入口文件行数整体下降，详见总览文档“最终行数对比”。

3. 回滚预案确认
- 维持“单职责提交”原则，按批次可独立 `git revert <commit>` 回退。
- 若某批次回退，保持其余批次提交不受影响；回退后必须重新执行修复流程。
- 若后续新增拆分任务在同域内失败超过 2 轮，按 Plan 5 规则继续拆小子任务后重试。
