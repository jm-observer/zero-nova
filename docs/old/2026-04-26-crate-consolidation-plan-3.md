# Plan 3: 合并应用门面逻辑

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 3: 将 nova-app 内容并入 nova-agent |
| 前置依赖 | Plan 2 |
| 执行状态 | 已完成（2026-04-27） |
| 本次目标 | 将 `nova-app` 中的源代码和依赖项迁移到 `nova-agent` 中，并删除 `nova-app` crate。 |
| 涉及文件 | `crates/nova-agent/Cargo.toml`, `crates/nova-agent/src/*`, `crates/nova-app/*`, `Cargo.toml` |
| 详细设计 | 1. 将 `nova-app/src` 下的模块移动到 `nova-agent/src/app/`（或直接放在根模块下，取决于职责）。<br>2. 合并 `nova-app/Cargo.toml` 中的依赖到 `nova-agent/Cargo.toml`。<br>3. 在 `nova-agent/src/lib.rs` 中重新导出 `AgentApplication` 等关键接口，保持向下兼容（如果可能）。<br>4. 更新所有依赖 `nova-app` 的 crate，改为依赖 `nova-agent`。<br>5. 从根目录 `Cargo.toml` 中移除 `nova-app`。 |
| 测试案例 | 1. 运行 `cargo check --workspace`。<br>2. 运行 `cargo test -p nova-agent`，确保应用启动相关测试通过。 |

## 实施结果

- 已在 `nova-agent` 新增 `src/facade/`，承载原 `nova-app` 的门面实现。
- 已在 `nova-agent/src/lib.rs` 重新导出 `AgentApplication`、`build_application`、`AppEvent` 等关键接口。
- `nova-gateway-core`、`nova-server-stdio`、`nova-server-ws` 已完成从 `nova-app` 到 `nova-agent` 的引用切换。
