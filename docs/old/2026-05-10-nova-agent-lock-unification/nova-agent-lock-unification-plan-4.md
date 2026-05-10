# Plan 4: 回归验证与约束固化

## 前置依赖
- Plan 3: 渐进迁移实现（核心模块先行）

## 本次目标
确保迁移后行为稳定，并将并发锁约束固化到项目实践：
- 完整执行修复流程并记录结果。
- 增补回归测试覆盖锁争用与会话状态一致性。
- 形成可持续检查项，防止引入同步阻塞锁回退。

## 涉及文件
- `crates/nova-agent/tests/integration/*.rs`
- `crates/nova-agent/src/**/*.rs`
- （可选）`docs/2026-05-10-nova-agent-lock-unification/nova-agent-lock-unification.md`

## 详细设计
1. 修复流程执行
- 在仓库根目录执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
- 任一步失败则修复后重跑完整循环。

2. 回归测试补强
- 新增/增强并发读写测试：
  - 多并发读取 + 间歇写入，验证最终状态一致。
  - 快速切换会话与 agent，验证 lineage 与 project dir 不串线。
- 保留已有集成测试作为主链路防回归保障。

3. 约束固化
- 在 code review 检查项中加入：
  - async 路径禁止新增 `std::sync::{RwLock, Mutex}`。
  - 锁持有区内禁止 IO/网络/复杂计算。
  - 锁获取不得使用 `unwrap/expect`（生产代码）。

4. 验收标准
- 修复流程三项全部通过。
- 回归测试稳定通过，无新增 flakiness。
- 文档中的 Plan 状态更新为“已完成”。

## 测试案例
- 正常路径：核心会话流程全量通过。
- 边界条件：高频并发请求下无死锁、无明显性能劣化。
- 异常路径：异常输入返回可诊断错误信息，日志无重复噪音。