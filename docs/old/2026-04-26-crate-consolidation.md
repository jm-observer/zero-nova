# Crate Consolidation Design

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-04-26；最后更新：2026-04-27 |
| 项目现状 | `nova-core`、`nova-conversation`、`nova-app` 已完成向 `nova-agent` 的合并。`nova-agent` 新增 `facade` 模块承载应用门面能力；`nova-gateway-core`、`nova-server-stdio`、`nova-server-ws` 已切换为直接依赖 `nova-agent`；workspace 已移除 `nova-app` 成员与依赖声明。 |
| 整体目标 | 将 `nova-core`、`nova-conversation` 和 `nova-app` 合并为一个统一的 `nova-agent` crate。简化依赖关系，降低外部调用方的集成成本。 |
| Plan 拆分 | 1. **Plan 1: 创建 nova-agent crate 并迁移核心逻辑** - 已完成。<br>2. **Plan 2: 迁移会话管理逻辑** - 已完成。<br>3. **Plan 3: 合并应用门面逻辑** - 已完成。<br>4. **Plan 4: 全局依赖更新与清理** - 已完成。 |
| 风险与待定项 | 1. 迁移后的 API 稳定性：后续新增能力时优先在 `nova-agent::facade` 保持接口稳定，避免下游重复适配。<br>2. 历史文档一致性：部分旧文档仍引用 `nova-app`，需在后续文档治理中统一更新。 |

## 当前任务进度（2026-04-27）

- 状态：`Plan 1 ~ Plan 4` 全部完成。
- 关键结果：
  - 新增 `crates/nova-agent/src/facade/*` 并在 `lib.rs` 统一导出门面接口。
  - 清理 `crates/nova-app` 代码与 workspace 配置。
  - 下游 crate 统一从 `nova-agent` 引入 `AgentApplication` 与 `build_application`。
- 验证结果：
  - `cargo clippy --workspace -- -D warnings` 通过。
  - `cargo fmt --check --all` 通过。
  - `cargo test --workspace` 通过。
