# Plan 1: 抽出 nova-agent-config 与共享配置模型

## 前置依赖

无

## 本次目标

建立 `nova-agent-config` crate，并把 `crates/nova-agent/src/config` 中的配置模型、raw schema、迁移、校验和路径解析迁入该 crate。完成后，配置加载不再属于 agent engine。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `Cargo.toml` | 修改 | 新增 workspace members 与 workspace dependencies |
| `crates/nova-agent-config/Cargo.toml` | 新增 | 配置 crate |
| `crates/nova-agent-config/src/models.rs` | 新增 | 从 `nova-agent/src/config/models.rs` 迁入并去除 engine 类型依赖 |
| `crates/nova-agent-config/src/loaders.rs` | 新增 | 从 `nova-agent/src/config/loaders.rs` 迁入 |
| `crates/nova-agent-config/src/validation.rs` | 新增 | 从 `nova-agent/src/config/validation.rs` 迁入 |
| `crates/nova-agent-config/src/lib.rs` | 新增 | 统一导出 config model、loader、validation |
| `crates/nova-agent/src/config/mod.rs` | 修改 | 短期 re-export `nova_agent_config::*` |
| `crates/nova-agent/Cargo.toml` | 修改 | 短期依赖 `nova-agent-config`，Plan 3 清理 |

## 详细设计

### 配置 crate 职责

`nova-agent-config`：

- TOML raw schema：`RawAppConfig`、`RawGatewayConfig`、`RawAgentSpec` 等。
- 稳定配置模型：`AppConfig`、`AgentSpec`、`ProviderConfig`、`RegisteredLlmConfig` 等。
- 配置迁移：legacy `system_prompt_template`、旧 trimmer 字段等。
- 配置校验：provider/llm/agent 引用关系、互斥字段、枚举值。
- 路径解析：`skills_dir()`、`prompts_dir()`、`project_context_file()`、`config_path()`。
- 环境变量覆盖：例如 `TAVILY_API_KEY`。

`nova-agent`：

- `PromptMaterial`、`TurnPromptMaterial`、`SystemPromptBuilder`。
- `SkillPackage`、`SkillRegistry`。
- `AgentRuntime`、`AgentRegistry`。
- built-in tools 与 runtime 能力。
- 短期可通过 `config` module re-export 兼容旧路径，但不能长期持有配置加载实现。

### 共享模型拆分决策

当前阻塞点：

- `AgentSpec` 依赖 `AgentModelOverride`。
- `ProviderConfig` / `RegisteredLlmConfig` 依赖 `ModelConfig`。
- `ResolvedAgentBinding` 当前直接返回 `crate::provider::ModelConfig`。

推荐短期方案：

1. 在 `nova-agent-config` 定义中立配置模型：
   - `ConfiguredAgentModel`
   - `ConfiguredModel`
   - `ConfiguredProvider`
2. 在 `nova-agent-loader` 中提供转换：
   - `ConfiguredAgentModel -> nova_agent::agent_catalog::AgentModelOverride`
   - `ConfiguredModel -> nova_agent::provider::ModelConfig`
3. 保持用户配置字段不变，只改变 Rust 内部类型归属。

这样可以避免 `nova-agent-config -> nova-agent -> nova-agent-config` 的循环依赖。

### 模型迁移范围

将以下类型迁入 `nova-agent-config`：

- `AppConfig`
- `PromptCompactionConfig`
- `OutboundContextHeaderConfig`
- `VoiceConfig`
- `ProviderConfig`
- `SearchConfig`
- `ToolConfig`
- `AgentSpec`
- `GatewayConfig`
- `TrimmerConfigToml`
- `SideChannelConfigToml`
- `LoopGuardConfigToml`
- `PromptDiagnosticsConfigToml`
- `ToolResultCompactionConfigToml`
- `RawAppConfig` 与所有 `Raw*` 类型

### 路径解析保留在 config crate

`AppConfig` 的以下方法随类型一起迁出：

- `skills_dir`
- `data_dir_path`
- `prompts_dir`
- `project_context_file`
- `config_path`

这些方法描述配置路径语义，不属于 engine。

### 兼容导出

为了降低一次性改动，`nova-agent/src/config/mod.rs` 可短期改为：

```rust
pub use nova_agent_config::*;
```

该 re-export 只作为迁移桥接，Plan 3 负责删除或保证 `nova-agent` 内部不再使用。

### 依赖方向

```text
nova-agent-config
    depends: anyhow, serde, serde_json, toml, log

nova-agent
    short-term: may re-export nova-agent-config for migration
    final: does not depend on nova-agent-config / nova-agent-loader / nova-skill-loader
```

## 测试案例

- 迁移前后现有 config 单测全部通过。
- `RawAppConfig::migrate` 对 legacy `system_prompt_template` 的行为保持一致。
- `AppConfig::validate` 对 provider/llm/agent 引用关系保持一致。
- `AppConfig::prompts_dir`、`skills_dir`、`project_context_file` 路径解析保持一致。
- `nova-agent-config` 单测覆盖默认值、路径解析、provider/llm 绑定校验。
