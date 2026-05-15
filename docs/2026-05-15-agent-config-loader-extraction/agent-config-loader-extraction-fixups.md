# Agent Config Loader Extraction — Fixups

## 时间

- 创建日期：2026-05-15
- 适用阶段：`2026-05-15-agent-config-loader-extraction` Plan 1–3 完成之后的后续修复

## 项目现状（Review 结论）

对 `2026-05-15-agent-config-loader-extraction` 三个 Plan 的实施结果做了完整 review，识别出与设计目标偏离的若干问题，按严重程度由高到低：

### 1. `nova-agent-config` 被「源码内联」绕过，实际未被任何代码使用

`crates/nova-agent/src/config.rs` 当前实现：

```rust
#[path = "../../nova-agent-config/src/loaders.rs"]
mod loaders;
#[path = "../../nova-agent-config/src/models.rs"]
mod models;
#[path = "../../nova-agent-config/src/validation.rs"]
mod validation;

pub use loaders::*;
pub use models::*;
```

- 这不是 `pub use nova_agent_config::*`，而是用 `#[path]` 把外部 crate 的源代码物理复制进 `nova-agent` 自己的 module tree。
- `crates/nova-agent/Cargo.toml` 并未声明 `nova-agent-config` 依赖。
- 整个 workspace 中 `rg "use nova_agent_config|nova_agent_config::" crates` 0 命中。`nova-agent-loader` 在 `Cargo.toml` 写了 `nova-agent-config` 依赖，但其源码统一用 `nova_agent::config::*`。
- 后果：
  - `nova_agent_config::AppConfig` 与 `nova_agent::config::AppConfig` 在 Rust 类型系统里是两个独立类型，nova-agent-config 编译产物是死代码。
  - 真正的"配置 schema 单一来源"未达成，Plan 1 验收标准（"`RawAppConfig` 与 `Raw*` TOML schema 不再位于 `nova-agent`"）只在文件位置上成立，编译边界上不成立。
  - 后续往哪个 crate 加字段、修改哪一份才生效，语义模糊。

Plan 3 「实施结果」自述「过渡门面，统一 `pub use nova_agent_config::*`」，与代码现实不符。

### 2. nova-cli 完全绕过 nova-agent-loader

- `crates/nova-cli/src/main.rs` 直接 `use nova_skill_loader::*` 并自定义 `convert_loaded_skills/convert_package/convert_tool_policy`，与 `crates/nova-agent-loader/src/skill_adapter.rs` 内三个函数一字不差重复。
- CLI 自行构造 `AgentConfig` / `OpenAiCompatClient` / `AgentRuntime`，没有走 `build_application`。
- 更严重的是 CLI 使用 `SystemPromptBuilder::new().with_tools(&tools).build()` 拼装 system prompt，不经过 `PromptMaterialLoader` / `SystemPromptBuilder::from_material`，这意味着 CLI 拿到的 prompt 缺少 agent prompt、project context、workflow prompt、agent catalog、environment snapshot 等所有 material loader 产物。CLI 与 Server 行为差异在本次重构后反而扩大。
- Plan 3 验收"CLI 和 server 都能通过同一 assembler 启动"未达成。

### 3. AgentTool / OrchestrateTaskTool 仍持有完整 `AppConfig` 并自建 runtime

- `crates/nova-agent/src/tool/builtin/agent.rs::AgentTool` 字段包含 `config: AppConfig`，`run_subagent` 内大段 `AgentConfig { trimmer, loop_guard, prompt_diagnostics, tool_result_compaction, ... }` 构造逻辑与 `nova-agent-loader/src/bootstrap.rs::build_application` 完全重复。
- Plan 3 设计目标"Agent tool 改为持有 agent catalog snapshot / prompt loader handle / descriptor factory / provider registry snapshot"未实现。AgentTool 仍然知道 `gateway.subagent_timeout_secs`、`gateway.loop_guard`、`gateway.tool_result_compaction` 等原始字段，DRY 与依赖收敛目标都未达成。
- `OrchestrateTaskTool` 同样持有 `AppConfig` 仅为了再 clone 给 AgentTool。

### 4. workspace reload 路径未回写配置快照

`nova-agent-loader/src/bootstrap.rs::ConfigBackedSessionPromptReloader`：

```rust
let reloaded_config = AppConfig::load_from_file(
    app_config_snapshot.config_path(),
    app_config_snapshot.config_dir.clone(),
)?;
// 仅用于本次构建 prompt，没有写回 self.config
```

- `Arc<RwLock<AppConfig>>` 是工作区里唯一共享的 config 句柄（被 `AgentWorkspaceService`、`AgentApplicationImpl`、Conversation 路径共用），但 reload 后没有更新。
- 用户改了 `.nova/config.toml` 后，prompt 会用新内容，但 `inspect_agent`、`resolve_agent_binding`、`config_snapshot` 接口仍返回旧值，出现行为不一致。
- 此外 reload 内部再次构造 `PromptMaterialLoader`，与 conversation/agent_tool 的另两条路径分别持有各自的 loader，没有共享 cache。

### 5. `nova-agent` 仍暴露 raw schema

- `crates/nova-agent/src/lib.rs` 第 26 行：`pub use config::RawAppConfig;`，把 raw TOML schema 类型继续作为 engine 的公开 API。Plan 1 验收"`RawAppConfig` 不再位于 nova-agent"在语义上未达成。
- 同文件第 7 行 `pub mod config;`，再加上 `#[path]` 内联，使外部仍然可以通过 `nova_agent::RawAppConfig`、`nova_agent::config::RawAppConfig` 访问 raw schema。

### 6. `nova-agent` 仍直接依赖 `toml`

- `crates/nova-agent/Cargo.toml` 仍保留 `toml = { workspace = true }`，被 `#[path]` 内联进来的 `loaders.rs` 使用。一旦修复 1 切到真依赖，该依赖应同步移除。

### 7. 测试夹具与集成测试硬编码 `nova_agent::config::*`

- `crates/nova-agent/tests/integration/session_project_runtime.rs` 与 `session_project_lineage.rs` 都 `use nova_agent::config::AppConfig`。短期可继续使用 re-export，但需保证 re-export 收敛后路径稳定。

## 整体目标

将本次抽出的三个 crate 真正落到「依赖图、类型系统、行为路径」三层一致：

```text
nova-cli / nova-server / deskapp
    ↓
nova-agent-loader
    ├── PromptMaterialLoader
    ├── AgentDescriptorFactory
    ├── SubagentRuntimeFactory（新增）
    └── build_application（唯一组装入口）
    ↓
nova-agent-config（唯一的 config schema 出处）
    ↓
nova-agent
    ├── AgentRuntime / AgentRegistry / SkillRegistry
    ├── SystemPromptBuilder
    └── built-in tools（不再持有 AppConfig）
```

期望结束状态：

- `nova_agent::config::*` 等同于 `pub use nova_agent_config::*`，不再有源码内联。
- `nova-agent` 不直接依赖 `toml`，不再 `pub use RawAppConfig`。
- `nova-cli` 通过 `nova-agent-loader` 统一组装；删除重复的 skill 转换函数。
- AgentTool / OrchestrateTaskTool 不再持有完整 AppConfig；runtime 构造逻辑收敛到 loader 层的 factory。
- `Arc<RwLock<AppConfig>>` reload 后回写，保证全局 single source of truth。

## Plan 拆分

| Plan | 描述 | 依赖 | 执行顺序 |
| --- | --- | --- | --- |
| Plan 1 | 修复 `nova-agent-config` 真正成为依赖；删除 `#[path]` 内联；清理 raw schema 暴露与 `toml` 直依赖 | 无 | 1 |
| Plan 2 | `nova-cli` 切换到 `nova-agent-loader` 统一组装路径；删除重复 skill 转换；CLI 走 `SystemPromptBuilder::from_material` | Plan 1 | 2 |
| Plan 3 | 抽 `SubagentRuntimeFactory`；AgentTool / OrchestrateTaskTool 不再持有完整 AppConfig | Plan 1 | 3（可与 Plan 2 并行） |
| Plan 4 | `ConfigStore` 抽象与 reload 回写；统一 PromptMaterialLoader 句柄 | Plan 1 | 4（可与 Plan 2/3 并行） |

## 风险与待定项

- Plan 1 改 `pub use nova_agent_config::*` 之后，`impl From<ConfiguredModel> for crate::provider::ModelConfig`（位于 `nova-agent::config`）等 orphan-rule 受限的转换实现必须保留在 `nova-agent` 一侧。设计需明确这些转换 impl 仍归属 `nova-agent`，与外部 schema 类型解耦。
- Plan 2 修复 CLI 组装路径会涉及 system prompt 内容变化，需要按 prompt 顺序对比新旧输出，避免破坏既有用户体验。建议先用 stream-json 模式抓取一次 prompt diff 后再合并。
- Plan 3 抽 `SubagentRuntimeFactory` 牵涉 `OpenAiCompatClient` 构造、`HttpClients` 共享、`build_provider_client` 重复调用等细节，需提前确认是否需要将 `HttpClients` 也下沉到 factory 内复用。
- Plan 4 `ConfigStore` 抽象在 loader crate 还是新建 `nova-agent-runtime-context` crate，需要权衡：短期方案放在 loader crate；如果后续 deskapp 也需要直接订阅 config 变更，再决定是否抽出。

## 非目标

- 不在本轮修复中改变 `.nova/config.toml` 用户可见字段。
- 不重写 `SystemPromptBuilder` / `PromptMaterial` 内部结构。
- 不在本轮引入 config 热加载 watcher；reload 仍由现有 RPC 触发。
- 不调整 SQLite session store、provider HTTP client 行为。

## 验收标准

- `cargo tree -p nova-agent` 不出现 `toml`、`nova-skill-loader`；可出现 `nova-agent-config`。
- `rg "#\[path = " crates/nova-agent` 0 命中。
- `rg "RawAppConfig" crates/nova-agent/src` 仅命中 re-export 之外的内部使用点，且 `nova-agent/src/lib.rs` 不再 `pub use config::RawAppConfig`。
- `rg "convert_loaded_skills|convert_tool_policy" crates/nova-cli` 0 命中。
- CLI 通过 `nova_agent_loader::build_application`（或共享 assembler）启动；CLI、stdio gateway、ws gateway 共用同一 assembler。
- `AgentTool` 字段不再包含 `config: AppConfig`；改持有 catalog/descriptor/factory handle。
- `Arc<RwLock<AppConfig>>` 在 reload 后被回写，`config_snapshot` 与 `inspect_agent` 返回最新内容。
- `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace` 全部通过。
