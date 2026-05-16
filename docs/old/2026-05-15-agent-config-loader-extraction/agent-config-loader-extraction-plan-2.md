# Plan 2: 抽出 nova-agent-loader 与资源加载 factory

## 前置依赖

Plan 1

## 本次目标

建立 `nova-agent-loader` crate，把 `PromptMaterialLoader`、skill adapter 和 agent descriptor factory 从 `nova-agent` 移入外部资源加载层，使 `nova-agent` 不再持有 prompt/project/workflow/skill discovery 的加载实现。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent-loader/Cargo.toml` | 新增 | 外部资源 loader/factory crate |
| `crates/nova-agent-loader/src/lib.rs` | 新增 | 暴露 prompt loader、skill adapter、descriptor factory |
| `crates/nova-agent-loader/src/prompt_loader.rs` | 新增 | 从 `nova-agent/src/app/prompt_loader.rs` 迁入 |
| `crates/nova-agent-loader/src/skill_adapter.rs` | 新增 | 从 `nova-agent/src/app/skill_adapter.rs` 迁入 |
| `crates/nova-agent-loader/src/descriptor_factory.rs` | 新增 | 构建 `AgentDescriptor` |
| `crates/nova-agent/src/app/prompt_loader.rs` | 删除或 re-export | 迁移桥接 |
| `crates/nova-agent/src/app/skill_adapter.rs` | 删除或 re-export | 迁移桥接 |
| `crates/nova-agent/Cargo.toml` | 修改 | 移除 `nova-skill-loader` 依赖 |

## 详细设计

### PromptMaterialLoader 输入收窄

不要让 loader 直接吃完整 `AppConfig`。新增小输入：

```rust
pub struct PromptLoaderConfig {
    pub config_dir: PathBuf,
    pub prompts_dir: PathBuf,
    pub project_context_file: Option<PathBuf>,
    pub developer_prompt_files: Vec<String>,
}
```

`impl From<&AppConfig> for PromptLoaderConfig` 放在 `nova-agent-loader` 或 `nova-agent-config`，调用方通过 config crate 生成。

### loader 输出

继续输出 `nova-agent` 的纯内容模型：

- `PromptMaterial`
- `TurnPromptMaterial`
- `SkillPackage`
- `AgentDescriptor`

这符合依赖方向：loader 依赖 agent，agent 不依赖 loader。

### AgentDescriptorFactory

新增 factory 集中构建 descriptor：

```rust
pub struct AgentDescriptorFactory {
    prompt_loader: PromptMaterialLoader,
}

impl AgentDescriptorFactory {
    pub async fn build_descriptor(
        &self,
        spec: &AgentSpec,
        binding: &ResolvedAgentBinding,
        material_inputs: AgentMaterialInputs,
        skills: &SkillRegistry,
    ) -> Result<AgentDescriptor>;
}
```

职责：

- 调用 `load_agent_material`。
- 调用 `SystemPromptBuilder::from_material` 构建启动期 system prompt。
- 转换 `ConfiguredAgentModel` 为 `AgentModelOverride`。
- 生成 `system_prompt_base`、`initial_template_vars`。

### Skill adapter

`skill_adapter` 从 `nova-agent::app` 移到 `nova-agent-loader`：

```text
nova-skill-loader::LoadedSkill
    -> nova-agent-loader::skill_adapter
    -> nova_agent::skill::SkillPackage
```

`nova-agent` 的 `Cargo.toml` 移除 `nova-skill-loader`，只保留 `SkillRegistry::from_packages`。

## 测试案例

- `PromptMaterialLoader` 现有单测迁移到 `nova-agent-loader`。
- 显式 `prompt_file` 缺失错误包含 agent id 和路径。
- 非 idle workflow stage 可加载并渲染模板变量。
- `convert_loaded_skills` 保持 `tool_policy`、`aliases`、`source_path`、`compat_mode` 字段完整。
- `AgentDescriptorFactory` 对同一 agent spec 生成的 `system_prompt_template` 与迁移前一致。
- `cargo tree -p nova-agent` 不包含 `nova-skill-loader`。
