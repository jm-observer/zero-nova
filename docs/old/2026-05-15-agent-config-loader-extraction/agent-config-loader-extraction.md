# Agent Config Loader Extraction

## 时间

- 创建日期：2026-05-15
- 最后更新：2026-05-15（Plan 3 收尾）

## 项目现状

`2026-05-14-agent-external-loading` 已经把 `nova-agent` 的 prompt builder 迁移到纯内容模型，并引入 `PromptMaterialLoader` 集中处理 agent prompt、developer project prompt、project context 和 workflow prompt。但边界仍然停在 `nova-agent::app` 内：

- `PromptMaterialLoader` 位于 `crates/nova-agent/src/app/prompt_loader.rs`，仍依赖 `AppConfig`、`AgentSpec` 和 `EnvironmentSnapshot`。
- skill loader adapter 位于 `crates/nova-agent/src/app/skill_adapter.rs`，使 `nova-agent` crate 仍保留 `nova-skill-loader` 依赖。
- `RawAppConfig`、`Raw*` TOML schema、迁移逻辑、环境变量覆盖、路径解析和验证都位于 `crates/nova-agent/src/config`。
- `build_application`、`ConversationService`、`AgentWorkspaceService` 和 Agent tool 仍在 `nova-agent::app` 中组装 loader、config、runtime 和外部服务。

当前 `nova-agent` 已经比迁移前更接近 engine，但还不是纯 engine：它仍知道 `.nova/config.toml`、`prompts/`、`skills/`、`workflow-stages.md`、`AGENTS.md` / `PROJECT.md` 等外部资源位置与兼容策略。

## 整体目标

继续收敛依赖方向，把配置加载和外部资源加载从 `nova-agent` 中拆出，使 `nova-agent` 只保留 agent engine、prompt 组装、skill registry、runtime 和工具执行能力。

目标分层：

```text
nova-cli / nova-server / deskapp
    ↓
nova-agent-loader
    ├── PromptMaterialLoader
    ├── SkillPackage adapter
    └── AgentDescriptorFactory
    ↓
nova-agent-config
    ├── RawAppConfig / Raw* TOML schema
    ├── AppConfig / AgentSpec / provider binding
    ├── migration / validation / path resolver
    └── environment overrides
    ↓
nova-agent
    ├── AgentRuntime
    ├── AgentRegistry
    ├── SkillRegistry
    ├── SystemPromptBuilder
    └── built-in tools
```

最终状态：

- `nova-agent` 不依赖 `nova-skill-loader`。
- `nova-agent` 不包含 `PromptMaterialLoader`、`RawAppConfig`、TOML 迁移和路径解析逻辑。
- `nova-agent-loader` 负责把 config、skill loader、prompt loader 的输出转换为 `nova-agent` 可消费模型。
- `nova-agent-config` 负责从文件或字符串加载、迁移、校验配置，并提供路径解析。
- 上层应用选择加载策略、创建 runtime、注册工具、启动服务。

## Plan 拆分

| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
| --- | --- | --- | --- | --- |
| Plan 1 | 抽出 `nova-agent-config` 与共享配置模型 | 无 | 1 | 已完成 |
| Plan 2 | 抽出 `nova-agent-loader` 与资源加载 factory | Plan 1 | 2 | 已完成 |
| Plan 3 | 上移应用组装、删除兼容层并验证依赖方向 | Plan 2 | 3 | 已完成 |

## 风险与待定项

- `AppConfig` 当前被 server、CLI、tool、workspace service 直接使用，抽出后需要保证公开 API 稳定，避免一次性改动过宽。
- `AgentModelOverride` 位于 `nova-agent::agent_catalog`，而 config schema 当前依赖它。Plan 1 需要决定把该类型移动到 config crate，还是先在 config crate 定义中立模型后转换。
- `ModelConfig` 位于 provider 模块，config crate 如果直接依赖 `nova-agent` 会造成反向依赖。应把 provider 配置模型移到独立 crate，或先在 `nova-agent-config` 中定义 `ConfiguredModel` 并在 loader 层转换。
- `build_application` 仍在 `nova-agent::app`，彻底上移到 `nova-cli` / `nova-server` 会牵涉大量测试。应先保持 facade，再逐步迁移调用者。
- `PromptMaterialLoader` 依赖 `EnvironmentSnapshot`，该类型是否留在 `nova-agent` 需要在 Plan 1 明确。短期可由 loader 调用 `nova-agent` 的环境快照类型，长期更适合抽出中立 `RuntimeEnvironmentSnapshot`。

## 非目标

- 不改变 `.nova/config.toml` 的用户可见格式。
- 不改变 prompt section 顺序、skill 注入策略、tool policy 语义。
- 不重构 SQLite session store、provider HTTP client、built-in read/write/edit 工具。
- 不在本设计中引入配置缓存或 watcher；后续可以在 loader 层扩展。

## 验收标准

- 新增或确定 `nova-agent-config`、`nova-agent-loader` 的 crate 边界和依赖方向。
- `RawAppConfig` 与 `Raw*` TOML schema 不再位于 `nova-agent`。
- `PromptMaterialLoader` 与 `skill_adapter` 不再位于 `nova-agent`。
- `nova-agent/src/config` 要么删除，要么仅保留短期 re-export 兼容层并标记迁移。
- `nova-agent` 不依赖 `nova-skill-loader`。
- `rg "RawAppConfig|PromptMaterialLoader|nova_skill_loader" crates/nova-agent/src` 只允许命中明确的兼容 re-export，最终阶段应无命中。
- `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace` 全部通过。
