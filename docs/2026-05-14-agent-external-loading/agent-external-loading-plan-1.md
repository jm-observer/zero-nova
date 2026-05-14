# Plan 1: 稳定注入契约与边界模型

## 前置依赖

无

## 本次目标

定义 `nova-agent` 的最终外部资源输入契约，使 agent engine 只消费已加载内容，不接收外部资源路径，也不决定 discovery/load 的错误策略。

本 Plan 只建立边界与模型，不要求一次性删除所有旧加载调用。完成后，后续 Plan 可以逐步把运行时调用链迁移到这些注入模型。

## 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/nova-agent/src/skill/types.rs` | 无改动 | `SkillPackage` 已定义，确认字段满足注入需求 |
| `crates/nova-agent/src/skill/registry.rs` | 新增方法 | 增加 `from_packages`、`replace_packages`、`extend_packages` |
| `crates/nova-agent/src/prompt/types.rs` | 新增类型 | 定义 `PromptMaterial`、`TurnPromptMaterial` |
| `crates/nova-agent/src/prompt/builder.rs` | 新增方法 | 增加 `from_material` 纯内容构建路径 |
| `crates/nova-agent/src/agent_catalog.rs` | 审查 | 确认 `AgentDescriptor` 字段覆盖 `LoadedAgentDescriptor` 需求 |

## 详细设计

### Agent 保留职责

`nova-agent` 继续负责 engine 内部能力：

- `SkillRegistry` 的 skill 存储、查询、alias 匹配、输入匹配、catalog prompt、active skill prompt、tool policy 派生。
- `SystemPromptBuilder` 的 section 组装、模板变量渲染、section 顺序、project instruction profile 过滤、prompt diagnostics。
- `AgentRegistry` 的 descriptor 注册和查找。
- `AgentRuntime` 的 turn 执行、tool 调度、provider 调用、loop guard、conversation 消息流处理。

### Agent 移出职责

`nova-agent` 不再负责外部资源 discovery/load：

- 不从目录递归扫描 skill。
- 不读取 `SKILL.md` 或 `skill.toml`。
- 不读取 agent `prompt_file` 或默认 `agent-<id>.md`。
- 不根据 `project_dir` 读取 developer project prompt 或 project context。
- 不根据 workflow stage 读取 workflow prompt 文件。
- 不记录包含外部资源路径的加载错误日志。

### 1.1 Skill 注入契约

当前 `SkillRegistry` 只有 `new()` 和 `load_from_dir*` / `load_single_skill*`（分别位于 `registry/discovery.rs` 和 `registry/parser.rs`），没有纯注入 API。

目标 API：

```rust
impl SkillRegistry {
    /// 从已加载的 SkillPackage 列表创建 registry。
    /// 重复 id/slug 时返回错误。
    pub fn from_packages(packages: Vec<SkillPackage>) -> anyhow::Result<Self>;

    /// 替换当前所有 packages（用于 hot-reload 场景）。
    pub fn replace_packages(&mut self, packages: Vec<SkillPackage>) -> anyhow::Result<()>;

    /// 追加新 packages，重复 id/slug 时返回错误。
    pub fn extend_packages(&mut self, packages: Vec<SkillPackage>) -> anyhow::Result<()>;
}
```

约束：

- `SkillPackage` 是 agent 内部消费模型，已定义在 `skill/types.rs`，不依赖 loader crate 类型。
- loader 到 `SkillPackage` 的转换发生在 app/bootstrap 或外层 adapter。
- 重复 slug/id 的处理策略：在注入时拒绝重复 id/slug，返回包含 key 的错误；loader 可提前校验，但 agent API 仍保持防御性。
- 兼容旧 `Skill` 列表只作为迁移字段存在，最终查询和 prompt 生成以 `SkillPackage` 为主。

实现要点：

```rust
// crates/nova-agent/src/skill/registry.rs

pub fn from_packages(packages: Vec<SkillPackage>) -> anyhow::Result<Self> {
    let mut registry = Self::new();
    registry.extend_packages(packages)?;
    Ok(registry)
}

pub fn extend_packages(&mut self, packages: Vec<SkillPackage>) -> anyhow::Result<()> {
    for pkg in packages {
        if self.packages.iter().any(|p| p.id == pkg.id || p.slug == pkg.slug) {
            anyhow::bail!(
                "Duplicate skill id/slug '{}' (slug='{}')", pkg.id, pkg.slug
            );
        }
        self.packages.push(pkg);
    }
    Ok(())
}

pub fn replace_packages(&mut self, packages: Vec<SkillPackage>) -> anyhow::Result<()> {
    self.packages.clear();
    self.skills.clear();
    self.extend_packages(packages)
}
```

### 1.2 Prompt 注入契约

当前 `PromptConfig`（`prompt/types.rs:119-156`）混合了"已加载内容"和"待加载路径"两类字段。需要新增两个纯内容模型，分离这两个关注点。

`PromptMaterial` 表示 agent descriptor 构建期的稳定内容：

```rust
// crates/nova-agent/src/prompt/types.rs

/// 启动期或 agent descriptor 构建所需的稳定 prompt 输入。
/// 所有字段均为已加载内容，不包含文件路径。
#[derive(Debug, Clone, Default)]
pub struct PromptMaterial {
    /// Agent 标识（用于日志和调试）
    pub agent_id: String,
    /// 已加载的 agent base prompt 内容
    pub agent_prompt: String,
    /// Orchestrator agent catalog 文本
    pub agent_catalog: Option<String>,
    /// 运行时环境快照
    pub environment_snapshot: Option<EnvironmentSnapshot>,
    /// 初始模板变量
    pub initial_template_vars: HashMap<String, String>,
    /// Skill 注入策略
    pub skill_injection_mode: SkillInjectionMode,
    /// 项目规则注入 profile
    pub project_instruction_profile: ProjectInstructionProfile,
    /// Tool 提示策略
    pub tool_guidance: ToolGuidanceMode,
}
```

`TurnPromptMaterial` 表示 turn 级动态内容：

```rust
/// 每轮 turn 可能变化的动态 prompt 输入。
/// 所有字段均为已加载内容，不包含文件路径。
#[derive(Debug, Clone, Default)]
pub struct TurnPromptMaterial {
    /// 已加载的开发项目提示词内容
    pub developer_project_prompt: Option<String>,
    /// 已加载的项目上下文内容
    pub project_context: Option<String>,
    /// 已加载的 workflow prompt 内容
    pub workflow_prompt: Option<String>,
    /// Turn 级模板变量（合并到 initial_template_vars 之上）
    pub turn_template_vars: HashMap<String, String>,
    /// 当前活跃 skill id
    pub active_skill: Option<String>,
}
```

目标 builder API：

```rust
impl SystemPromptBuilder {
    /// 从纯内容模型构建 prompt，不执行任何文件 IO。
    pub fn from_material(
        material: &PromptMaterial,
        turn_material: &TurnPromptMaterial,
        skills: &SkillRegistry,
    ) -> Self {
        // 合并 template_vars: initial + turn
        let mut vars = material.initial_template_vars.clone();
        vars.extend(turn_material.turn_template_vars.clone());

        let mut builder = Self::new();

        // 1. Base section: agent prompt + template rendering
        let rendered_prompt = if vars.is_empty() {
            material.agent_prompt.clone()
        } else {
            TemplateContext::render(&material.agent_prompt, &vars)
        };
        if !rendered_prompt.is_empty() {
            builder = builder.base_section(&rendered_prompt);
        }

        // 2. Behavior guards
        builder = builder.behavior_guards_section();

        // 3. Skill section
        let skill_prompt = match material.skill_injection_mode {
            SkillInjectionMode::Catalog => skills.generate_catalog_prompt(),
            SkillInjectionMode::ActiveFull => {
                skills.generate_contextual_prompt(turn_material.active_skill.as_deref())
            }
            SkillInjectionMode::Full => skills.generate_full_prompt(),
        };
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        // 4. Developer project prompt section
        if let Some(ref content) = turn_material.developer_project_prompt {
            builder = builder.developer_project_prompt_section(
                filter_project_instruction_by_profile(
                    content,
                    material.project_instruction_profile,
                ),
            );
        }

        // 5. Project context section
        if let Some(ref content) = turn_material.project_context {
            builder = builder.project_context_section(content);
        }

        // 6. Environment snapshot
        if let Some(ref env) = material.environment_snapshot {
            builder = builder.environment_snapshot(env);
        }

        // 7. Agent catalog section
        if let Some(ref catalog) = material.agent_catalog {
            if !catalog.is_empty() {
                builder = builder.agent_catalog_section(catalog);
            }
        }

        // 8. Workflow prompt section
        if let Some(ref content) = turn_material.workflow_prompt {
            if !content.is_empty() {
                builder = builder.workflow_section(content);
            }
        }

        builder
    }
}
```

约束：

- material 只包含内容或已经结构化的数据，不包含文件路径。
- builder 不再是 async 文件加载入口。
- `PromptLoadContext` 可作为迁移桥接保留，但不能成为最终 API 的核心模型。
- `from_material` 与现有 `from_config_async` 的 section 注入顺序和 filter 逻辑保持一致。

### 1.3 Agent descriptor 注入契约

当前 `AgentDescriptor`（`agent_catalog.rs:13-27`）已基本满足需求。分析现有字段：

```rust
pub struct AgentDescriptor {
    pub id: String,                                     // ✅ 保留
    pub display_name: String,                           // ✅ 保留
    pub description: String,                            // ✅ 保留
    pub aliases: Vec<String>,                           // ✅ 保留
    pub system_prompt_template: String,                 // ✅ 启动期完整构建的 prompt
    pub system_prompt_base: String,                     // ✅ 基础 prompt（用于 turn 重建）
    pub initial_template_vars: HashMap<String, String>, // ✅ 保留
    pub tool_whitelist: Option<Vec<String>>,            // ✅ 保留
    pub model_config: Option<AgentModelOverride>,       // ✅ 保留
    pub provider_id: String,                            // ✅ 保留
    pub llm_id: String,                                 // ✅ 保留
    pub enable_project_developer_prompt: bool,          // ✅ 保留
}
```

外层 factory 负责把配置和 prompt material 转成 loaded descriptor：

```text
AgentSpec + AppConfig + ProviderBinding + PromptMaterial
    -> AgentDescriptorFactory::build(...)
    -> AgentDescriptor
    -> AgentRegistry::register(...)
```

`AgentRegistry` 不解析 `prompt_file`，也不做默认 prompt 文件 fallback。所有 prompt 内容已在 factory 阶段由 `PromptMaterialLoader` 加载完成。

### 1.4 `from_config_async` 与 `from_material` 行为对照

确保 `from_material` 完全覆盖 `from_config_async` 的 section 注入路径：

| Section | `from_config_async` 来源 | `from_material` 来源 | 对应 |
|---------|------------------------|---------------------|------|
| Base | `config.agent_prompt` + `template_vars` | `material.agent_prompt` + merged vars | ✅ |
| BehaviorGuards | 常量 `BEHAVIOR_GUARDS` | 同左 | ✅ |
| Skill | `skills.*` + `config.skill_injection` | `skills.*` + `material.skill_injection_mode` | ✅ |
| DeveloperProjectPrompt | `config.developer_project_prompt_content` 或 fallback IO | `turn_material.developer_project_prompt` | ✅ 无 IO |
| ProjectContext | `config.project_context_content` 或 fallback IO | `turn_material.project_context` | ✅ 无 IO |
| Environment | `config.environment` | `material.environment_snapshot` | ✅ |
| AgentCatalog | `config.agent_catalog` | `material.agent_catalog` | ✅ |
| Workflow | `config.workflow_prompt_path` + IO | `turn_material.workflow_prompt` | ✅ 无 IO |

## 数据流

```text
AppConfig / AgentSpec / runtime turn context
    -> 外部 loader/factory 解析路径和内容
    -> SkillPackage / PromptMaterial / TurnPromptMaterial / AgentDescriptor
    -> nova-agent registry / prompt builder / runtime
```

## 迁移策略

1. 先新增 `PromptMaterial`、`TurnPromptMaterial` 类型到 `prompt/types.rs`。
2. 新增 `SystemPromptBuilder::from_material`，复用现有 section builder 方法。
3. 新增 `SkillRegistry::from_packages` / `extend_packages` / `replace_packages`。
4. 编写单元测试验证 `from_material` 与 `from_config_async` 输出一致。
5. 保留旧 `load_from_dir*`、`from_config_async`，标记为迁移桥接。
6. 后续 Plan 把运行时调用点迁到注入 API。
7. 所有调用点迁移完成后再删除旧 API 和 loader 依赖。

## 测试案例

- 正常路径：传入两个 `SkillPackage` 后通过 `from_packages` 创建 registry，catalog prompt、active skill prompt、tool policy 与手动 `push` 结果一致。
- 正常路径：传入完整 `PromptMaterial` 和 `TurnPromptMaterial` 后，`from_material` 生成的 system prompt 与同等输入下 `from_config_async` 的结果一致。
- 正常路径：`from_material` 正确合并 `initial_template_vars` 和 `turn_template_vars`，turn 变量覆盖初始变量。
- 边界条件：空 skill 列表生成空 skill section，不影响 agent 启动。
- 边界条件：developer prompt、project context、workflow prompt 均为 `None` 时，不生成对应 section。
- 边界条件：`PromptMaterial` 的 `agent_prompt` 为空时只生成 BehaviorGuards 及后续 section。
- 异常场景：重复 skill id/slug 被 `extend_packages` 明确拒绝，错误信息包含重复 key，不包含文件系统路径依赖。
