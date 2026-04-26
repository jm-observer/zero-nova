# Plan 1: 创建 nova-agent crate 并迁移核心逻辑

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 1: 重命名 nova-core 为 nova-agent 并同步基础配置 |
| 前置依赖 | 无 |
| 本次目标 | 将 `crates/nova-core` 重命名为 `crates/nova-agent`，更新其 `Cargo.toml` 和全局 `Cargo.toml`，确保项目仍能编译通过。 |
| 涉及文件 | `Cargo.toml`, `crates/nova-core/*`, `crates/nova-agent/*`, 以及所有依赖 `nova-core` 的 `Cargo.toml` |
| 详细设计 | 1. 将目录 `crates/nova-core` 重命名为 `crates/nova-agent`。<br>2. 修改 `crates/nova-agent/Cargo.toml` 中的 `name = "nova-agent"`。<br>3. 在根目录 `Cargo.toml` 中，将 `members` 中的 `crates/nova-core` 改为 `crates/nova-agent`，将 `workspace.dependencies` 中的 `nova-core` 改为 `nova-agent`。<br>4. 遍历所有 crate，将依赖中的 `nova-core` 替换为 `nova-agent`。<br>5. 执行 `cargo check` 验证。 |
| 测试案例 | 1. 运行 `cargo check --workspace` 确保无编译错误。<br>2. 运行 `cargo test -p nova-agent` 确保核心逻辑测试通过。 |
