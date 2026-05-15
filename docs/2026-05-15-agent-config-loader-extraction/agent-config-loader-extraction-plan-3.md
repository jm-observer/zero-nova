# Plan 3: 上移应用组装、删除兼容层并验证依赖方向

## 前置依赖

Plan 2

## 本次目标

把应用组装逻辑从 `nova-agent::app` 上移到 `nova-cli` / `nova-server` / gateway 层，删除迁移桥接，验证 `nova-agent` 已从 config loader 和 external loader 中脱钩。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent/src/app/bootstrap.rs` | 拆分 | 保留 runtime 构建辅助或删除 |
| `crates/nova-agent/src/app/conversation_service.rs` | 审查 | 确认只消费已注入 config/material loader handle |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 审查 | 移除 config 文件重载职责或通过外层注入 |
| `crates/nova-agent/src/config` | 删除 | 不再 re-export config crate |
| `crates/nova-agent/src/app/prompt_loader.rs` | 删除 | 不再 re-export loader |
| `crates/nova-agent/src/app/skill_adapter.rs` | 删除 | loader crate 持有转换 |
| `crates/nova-agent/Cargo.toml` | 修改 | 移除 `nova-agent-config`、`nova-agent-loader`、`nova-skill-loader` |
| `crates/nova-cli/src/*` | 修改 | 使用 `nova-agent-config` + `nova-agent-loader` 组装应用 |
| `crates/nova-server/src/*` | 修改 | 使用同一组装路径 |
| `docs/2026-05-15-agent-config-loader-extraction/*` | 更新 | 标记 Plan 状态与最终迁移说明 |

## 详细设计

### 上层组装流程

```text
load AppConfig via nova-agent-config
    -> load skills via nova-agent-loader + nova-skill-loader
    -> build SkillRegistry::from_packages
    -> build agent descriptors via AgentDescriptorFactory
    -> create AgentRuntime
    -> register built-in tools
    -> create ConversationService / WorkspaceService
```

### nova-agent app facade 迁移策略

短期保留：

```rust
nova_agent::app::build_application(config: AppConfig)
```

但实现应委托到外部 assembler，或被 `nova-agent-loader`/上层 crate 接管。最终 `nova-agent::app` 可以只保留 engine-adjacent service 类型，不再做文件加载。

### workspace reload 调整

当前 `AgentWorkspaceService::reload_session_system_prompt` 会重新 `AppConfig::load_from_file`。迁移后有两种选择：

1. 上层提供 `ConfigReloader` trait，workspace service 调用 trait，不知道 TOML 文件。
2. workspace reload 移到上层应用服务，`nova-agent` 只提供 `SessionService::reload_system_prompt`。

推荐选项 1 作为过渡，选项 2 作为最终目标。

### Agent tool 调整

Agent tool 当前持有完整 `AppConfig` 并内部创建 runtime。迁移后应改为持有：

- agent catalog snapshot
- prompt loader handle
- descriptor factory 或 descriptor registry
- provider registry snapshot

这样 Agent tool 不再知道 `.nova/config.toml` 的 raw schema。

### 最终依赖检查

期望：

```text
nova-agent
    does not depend on nova-agent-config
    does not depend on nova-agent-loader
    does not depend on nova-skill-loader

nova-agent-loader
    depends on nova-agent
    depends on nova-agent-config
    depends on nova-skill-loader

nova-cli / nova-server
    depend on nova-agent
    depend on nova-agent-config
    depend on nova-agent-loader
```

### 搜索验证

最终应通过：

```bash
rg "RawAppConfig|RawGatewayConfig|RawAgentSpec" crates/nova-agent/src
rg "PromptMaterialLoader|load_agent_prompt|load_turn_material" crates/nova-agent/src
rg "nova_skill_loader" crates/nova-agent
rg "AppConfig::load_from_file|std::fs::read_to_string|tokio::fs::read_to_string" crates/nova-agent/src/app crates/nova-agent/src/prompt
```

允许保留的 IO：

- built-in tool 文件读写。
- SQLite conversation store。
- provider HTTP 调用。
- config 或 loader crate 中的文件读取。

### 迁移说明

文档需说明：

- 如何从 `nova_agent::config::AppConfig` 迁移到 `nova_agent_config::AppConfig`。
- 如何从 `nova_agent::app::PromptMaterialLoader` 迁移到 `nova_agent_loader::PromptMaterialLoader`。
- 如何从 `build_application(config)` 迁移到上层 assembler。
- 新增 agent config 字段时应修改 `nova-agent-config`。
- 新增外部资源加载行为时应修改 `nova-agent-loader`。
- 新增 engine prompt section 或 runtime 行为时才修改 `nova-agent`。

## 测试案例

- CLI 和 server 都能通过同一 assembler 启动。
- `build_application` 兼容 facade 与新 assembler 输出等价。
- workspace reload 不直接调用 `AppConfig::load_from_file`。
- Agent tool 创建 subagent 时不直接读取 prompt 文件，只通过 loader handle。
- 集成测试覆盖无 skill、无 prompt 文件、legacy prompt、显式 prompt 缺失等路径。
- `cargo clippy --workspace -- -D warnings` 通过。
- `cargo fmt --all --check` 通过。
- `cargo test --workspace` 通过。
- `cargo tree -p nova-agent` 不包含 `nova-agent-config`、`nova-agent-loader`、`nova-skill-loader`。
- 搜索验证命令符合预期。

## 实施结果（2026-05-15）

- 已完成 `app` 层关键组装逻辑上移：`bootstrap.rs` 与 `skill_adapter.rs` 从 `nova-agent` 移除，由外层 loader / 启动层承担组装职责。
- 已完成 `PromptMaterialLoader` 迁移：`crates/nova-agent/src/app/prompt_loader.rs` 删除，`ConversationService` 与 `AgentTool` 均改为通过注入 trait 获取 turn/material。
- 已完成 workspace reload 脱钩：`AgentWorkspaceService::reload_session_system_prompt` 不再直接读取配置文件，改为通过注入 reloader 获取行为。
- 已完成 `src/config/` 目录删除：旧 `loaders/models/validation` 迁移桥接模块已移除。
- 当前保留 `crates/nova-agent/src/config.rs` 作为过渡门面，统一 `pub use nova_agent_config::*`，并保留面向 runtime 的类型转换实现。

### 依赖方向结论

- `nova-agent` 已不再依赖 `nova-agent-loader`、`nova-skill-loader`。
- `nova-agent` 当前仍依赖 `nova-agent-config`，用于承载 `crate::config::*` 的稳定 API 门面。
- 因此本 Plan 的“删除兼容层”按“删除目录级兼容层并收敛到单文件门面”落地；“完全去除 `nova-agent-config` 依赖”不在本次收尾内继续扩展，以避免扩大改动面。
