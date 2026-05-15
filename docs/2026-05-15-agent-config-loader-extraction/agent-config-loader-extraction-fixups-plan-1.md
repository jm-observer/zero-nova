# Plan 1: 修复 `nova-agent-config` 真正成为依赖

## 前置依赖

无

## 本次目标

把 `crates/nova-agent/src/config.rs` 里的 `#[path = "..."]` 源码内联改为对 `nova-agent-config` crate 的真正依赖与 `pub use`，让 `nova_agent::config::*` 与 `nova_agent_config::*` 是同一组类型。同时清理 raw schema 暴露和不再需要的 `toml` 直接依赖。

完成后：

- `nova-agent` 通过 Cargo.toml 真正依赖 `nova-agent-config`。
- `nova_agent_config` crate 在编译产物层面被实际使用。
- `nova_agent::config::AppConfig == nova_agent_config::AppConfig`（同一类型）。
- `nova-agent` 不直接依赖 `toml`，不再 `pub use RawAppConfig`。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent/Cargo.toml` | 修改 | 新增 `nova-agent-config = { workspace = true }`；移除 `toml` 直接依赖 |
| `crates/nova-agent/src/config.rs` | 修改 | 删除 `#[path]` 块，改为 `pub use nova_agent_config::*` 并保留 `impl From` 转换 |
| `crates/nova-agent/src/lib.rs` | 修改 | 删除 `pub use config::RawAppConfig;`；保留 `pub mod config` 作为短期 re-export 门面 |
| `crates/nova-agent-loader/src/bootstrap.rs` | 修改 | 把 `use nova_agent::config::*` 改为 `use nova_agent_config::*`（可分两步：先保留 nova_agent::config 路径，确认编译，再切换）|
| `crates/nova-agent-loader/src/prompt_loader.rs` | 修改 | 同上 |
| `crates/nova-agent-loader/src/descriptor_factory.rs` | 修改 | 同上 |
| `Cargo.toml`（workspace 根） | 检查 | 确认 `nova-agent-config` 已在 `workspace.dependencies`；如未在则补 |

## 详细设计

### 改造 `nova-agent/src/config.rs`

目标内容：

```rust
//! Re-export of the shared config schema and helpers from `nova-agent-config`.
//!
//! This module exists to:
//! 1. Provide a stable `crate::config::*` import path within `nova-agent`.
//! 2. Host orphan-rule-bound `From` conversions into engine-side types
//!    (`provider::ModelConfig`, `agent_catalog::AgentModelOverride`).

pub use nova_agent_config::*;

impl From<ConfiguredModel> for crate::provider::ModelConfig {
    fn from(value: ConfiguredModel) -> Self { /* unchanged */ }
}

impl From<ConfiguredAgentModel> for crate::agent_catalog::AgentModelOverride {
    fn from(value: ConfiguredAgentModel) -> Self { /* unchanged */ }
}
```

关键点：

- 删除 `#[path = "../../nova-agent-config/src/..."]` 三行 `mod` 声明。
- `pub use nova_agent_config::*;` 必须能覆盖原本由 `loaders::*` / `models::*` 暴露的全部符号。如果 `nova_agent_config::lib.rs` 已经 `pub use loaders::*; pub use models::*;`（当前实际就是），可以直接通配 re-export。
- 两个 `impl From` 必须保留在本 crate：`provider::ModelConfig` 与 `agent_catalog::AgentModelOverride` 是 `nova-agent` 的本地类型，根据 orphan rule，`impl From<ConfiguredX> for LocalY` 必须由本 crate 提供。

### Cargo.toml 修改

`crates/nova-agent/Cargo.toml`：

```toml
[dependencies]
# ... existing
nova-agent-config = { workspace = true }
# 移除：toml = { workspace = true }
```

确认 `toml` 没有被 `nova-agent` 内部其他模块使用：

- 验证命令：`rg "toml::|use toml" crates/nova-agent/src`
- 预期 0 命中。如有命中，先处理（搬到 nova-agent-config，或保留 toml 依赖并在 Plan 备注）。

`Cargo.toml`（workspace 根）：

- 检查 `[workspace.dependencies]` 已有 `nova-agent-config = { path = "crates/nova-agent-config" }`；缺则补齐。

### `lib.rs` 清理

`crates/nova-agent/src/lib.rs`：

- 删除 `pub use config::RawAppConfig;`（line 26）。`RawAppConfig` 是 raw TOML schema，按 Plan 1 设计不应作为 engine 的公开 API。
- `pub mod config;` 保留——它现在仅是一个 re-export 门面 + orphan-impl 容器。
- 检查并移除其他可能从 `config` 模块暴露的 raw schema 类型（`RawGatewayConfig`、`RawAgentSpec` 等若有 `pub use`）。

### loader crate 切换 import 路径

`crates/nova-agent-loader/src/{bootstrap,prompt_loader,descriptor_factory,skill_adapter}.rs`：

- 把所有 `use nova_agent::config::{...}` 替换为 `use nova_agent_config::{...}`。
- 该 crate `Cargo.toml` 已经声明 `nova-agent-config`，无需新增。
- 注意 `ResolvedAgentBinding`、`AgentSpec`、`AppConfig` 等都需要切换。
- 注意 `impl From<ConfiguredModel> for ModelConfig`、`impl From<ConfiguredAgentModel> for AgentModelOverride` 这两个转换仍走 `nova-agent` 提供的 impl，调用语义 `binding.model_config.clone().into()` 不变。

完成此步后，loader 与 `nova-agent` 双方都用 `nova-agent-config` 作为类型源头，编译验证：`cargo build -p nova-agent-loader` 应当成功。

### 类型一致性自检

迁移完成后执行：

```bash
rg "#\[path = " crates/nova-agent
rg "use nova_agent_config" crates
rg "use nova_agent::config" crates
```

预期：

- `#[path]` 0 命中。
- `nova_agent_config` import 在 loader、cli、server 都能出现（cli/server 切换由 Plan 2 落地，本 Plan 不强制）。
- `nova_agent::config` import 在过渡期仍允许（指向同一类型）。

### 兼容性说明

- 由于 `nova_agent::config::*` 现在 = `nova_agent_config::*`，CLI / server / 测试中所有 `use nova_agent::config::AppConfig` 仍可编译，无需立刻全量替换。Plan 2/3 会负责把上层切换到直接使用 `nova-agent-config`。
- 测试夹具 `tests/integration/session_project_*.rs` 不需要在本 Plan 修改。

## 测试案例

### 编译与依赖图

- `cargo build --workspace`：通过。
- `cargo tree -p nova-agent`：
  - 包含 `nova-agent-config`。
  - 不再包含 `toml`（顶层）。
- `cargo tree -p nova-agent-config`：仅包含 `serde / serde_json / toml / log / anyhow`。

### 类型同一性

- 新增简单单测（`crates/nova-agent/src/config.rs` 内 `#[cfg(test)] mod tests`）：

```rust
#[test]
fn config_type_is_re_exported() {
    fn _accepts(_: nova_agent_config::AppConfig) {}
    _accepts(crate::config::AppConfig::new("/tmp".into()));
}
```

  该测试若通过则证明两条路径解析为同一类型；若仍是 `#[path]` 内联，编译会因 type mismatch 失败。

### 行为不回归

- `cargo test -p nova-agent`：所有现有 config 单测、prompt 单测、tool 单测通过。
- `cargo test -p nova-agent-config`：通过。
- `cargo test -p nova-agent-loader`：通过。

### 公开 API

- `rg "pub use config::RawAppConfig" crates/nova-agent/src/lib.rs`：0 命中。
- `rg "RawAppConfig" crates/nova-agent/src`：仅命中内部使用点（如 `Default` impl 内部或测试），不在 lib.rs 顶层 re-export。

### 异常路径

- 移除 `toml` 后 `cargo build -p nova-agent` 通过；如果失败，定位到具体未迁移的 `toml::` 用法，决定是搬到 nova-agent-config 还是单独保留 `toml`。
