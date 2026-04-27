# Plan 4: 全局清理与目录结构优化

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 4: 彻底移除旧 crate 目录并优化新目录结构 |
| 前置依赖 | Plan 3 |
| 执行状态 | 已完成（2026-04-27） |
| 本次目标 | 清理物理目录，调整 `nova-agent` 内部目录结构使其更清晰，并进行最终的编译和测试验证。 |
| 涉及文件 | `crates/nova-core`, `crates/nova-conversation`, `crates/nova-app`, `crates/nova-agent/src/*` |
| 详细设计 | 1. 删除已合并的空目录：`crates/nova-core`, `crates/nova-conversation`, `crates/nova-app`。<br>2. 优化 `nova-agent` 的模块组织，例如：<br>   - `src/core/`: 原 `nova-core` 逻辑<br>   - `src/conversation/`: 原 `nova-conversation` 逻辑<br>   - `src/facade/`: 原 `nova-app` 逻辑<br>3. 确保 `lib.rs` 的导出逻辑简洁明了。<br>4. 执行最终的修复流程。 |
| 测试案例 | 1. `cargo clippy --workspace`<br>2. `cargo fmt --check --all`<br>3. `cargo test --workspace` |

## 实施结果

- `nova-app` 已从 workspace `members` 与 `workspace.dependencies` 中移除。
- `crates/nova-app` 代码文件已清理，`nova-agent` 的目录职责已按 `conversation + facade + core模块` 收拢。
- 最终修复流程已通过：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`
