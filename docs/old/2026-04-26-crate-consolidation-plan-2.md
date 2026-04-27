# Plan 2: 迁移会话管理逻辑

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 2: 将 nova-conversation 内容并入 nova-agent |
| 前置依赖 | Plan 1 |
| 执行状态 | 已完成（2026-04-27） |
| 本次目标 | 将 `nova-conversation` 中的源代码和依赖项迁移到 `nova-agent` 中，并删除 `nova-conversation` crate。 |
| 涉及文件 | `crates/nova-agent/Cargo.toml`, `crates/nova-agent/src/*`, `crates/nova-conversation/*`, `Cargo.toml` |
| 详细设计 | 1. 将 `nova-conversation/src` 下的所有模块移动到 `nova-agent/src/conversation/`（或其他合适目录）。<br>2. 合并 `nova-conversation/Cargo.toml` 中的依赖到 `nova-agent/Cargo.toml`。<br>3. 在 `nova-agent/src/lib.rs` 中导出迁移后的模块。<br>4. 更新所有依赖 `nova-conversation` 的 crate，改为依赖 `nova-agent` 并调整导入路径。<br>5. 从根目录 `Cargo.toml` 中移除 `nova-conversation`。 |
| 测试案例 | 1. 运行 `cargo check --workspace`。<br>2. 运行 `cargo test -p nova-agent`，确保会话相关测试通过。 |

## 实施结果

- `nova-agent/src/conversation` 已承载会话管理逻辑，并在 `lib.rs` 对外导出。
- 下游模块已通过 `nova-agent` 访问会话能力，不再依赖独立 `nova-conversation` crate。
