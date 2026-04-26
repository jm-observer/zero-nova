# Plan 4: 全局清理与目录结构优化

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 4: 彻底移除旧 crate 目录并优化新目录结构 |
| 前置依赖 | Plan 3 |
| 本次目标 | 清理物理目录，调整 `nova-agent` 内部目录结构使其更清晰，并进行最终的编译和测试验证。 |
| 涉及文件 | `crates/nova-core`, `crates/nova-conversation`, `crates/nova-app`, `crates/nova-agent/src/*` |
| 详细设计 | 1. 删除已合并的空目录：`crates/nova-core`, `crates/nova-conversation`, `crates/nova-app`。<br>2. 优化 `nova-agent` 的模块组织，例如：<br>   - `src/core/`: 原 `nova-core` 逻辑<br>   - `src/conversation/`: 原 `nova-conversation` 逻辑<br>   - `src/facade/`: 原 `nova-app` 逻辑<br>3. 确保 `lib.rs` 的导出逻辑简洁明了。<br>4. 执行最终的修复流程。 |
| 测试案例 | 1. `cargo clippy --workspace`<br>2. `cargo fmt --check --all`<br>3. `cargo test --workspace` |
