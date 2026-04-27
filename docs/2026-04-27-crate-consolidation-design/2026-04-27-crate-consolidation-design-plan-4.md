# Plan 4: 回归验证、发布前检查与回滚方案

## Plan 编号与标题
- Plan 4: 回归验证与回滚

## 前置依赖
- Plan 3

## 本次目标
- 在完整迁移后执行统一修复流程并达到全绿。
- 输出最小可执行的回滚策略，降低上线风险。
- 明确发布前校验项和执行顺序。

## 涉及文件
- 修改: CI 工作流（如受 crate 名变化影响）
- 修改: 启动脚本与文档（若有 crate 包名硬编码）
- 新增/修改: `docs/2026-04-27-crate-consolidation-design/*`（实施记录）

## 详细设计
- 强制验证循环（每次变更后执行）:
  1. `cargo clippy --workspace -- -D warnings`
  2. `cargo fmt --check --all`
  3. `cargo test --workspace`
- 回滚策略:
  - 回滚点 A: Plan 1 完成后，如发现入口不兼容，仅回退 `nova-server` 新增与 workspace member 调整。
  - 回滚点 B: Plan 2 完成后，如通道行为回归失败，可恢复旧 `channel-*` crate 并切回原引用。
  - 回滚点 C: Plan 3 完成后，如构建图异常，可恢复 root `Cargo.toml` 的成员与内部依赖映射。
- 发布前检查:
  - 本地验证两个目标平台构建命令是否可通过。
  - 验证外部调用命令（尤其 `cargo run -p ... --bin ...`）是否更新完整。

## 测试案例
- 正常路径:
  - 三步修复流程连续通过。
  - stdio 与 ws 端到端 smoke test 通过。
- 边界条件:
  - 只构建单 bin（stdio 或 ws）仍可独立通过。
- 异常场景:
  - 人为制造 ws 非法消息/stdio 非法输入，确认日志和错误边界清晰。
