# Plan 5: 旧 API 清理、验证与迁移说明

## 前置依赖

Plan 4

## 本次目标

删除或隔离 agent 内部外部资源加载 API，验证依赖方向符合"外层加载、agent 注入"，并补齐迁移说明和验收测试。

本 Plan 是收口阶段，不再引入新的架构概念。

## 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/nova-agent/src/skill/registry.rs` | 审查 | 确认 `mod discovery; mod parser;` 已删除（Plan 2） |
| `crates/nova-agent/src/prompt/builder.rs` | 删除 | 移除 `from_config_async`，`#[deprecated]` 标记不再需要 |
| `crates/nova-agent/src/prompt/types.rs` | 清理 | 移除或标记 `PromptConfig`、`PromptLoadContext` |
| `crates/nova-agent/src/prompt/context.rs` | 审查 | `load_developer_project_prompt_async`、`load_project_context_with_config_async` 保留（被 `PromptMaterialLoader` 调用） |
| `crates/nova-agent/src/prompt/workflow.rs` | 审查 | `WorkflowStagePrompts` 保留（被 `PromptMaterialLoader` 调用） |
| `crates/nova-agent/src/app/bootstrap.rs` | 审查 | 确认无直接 prompt 文件 IO |
| `crates/nova-agent/src/app/conversation_service.rs` | 审查 | 确认无直接 prompt 文件 IO |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 审查 | 确认 `load_agent_prompt_for_reload` 已删除 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 审查 | 确认 `AgentTool::load_agent_prompt` 已删除 |

## 详细设计

### 清理目标

| 项目 | 状态 | 说明 |
|------|------|------|
| `nova-agent` 不依赖 `nova-skill-loader`（核心层） | Plan 2 已完成 | `src/skill/` 无 `nova_skill_loader` import |
| `SkillRegistry` 不暴露目录加载 API | Plan 2 已完成 | `load_from_dir*`、`load_single_skill*` 已删除 |
| `SystemPromptBuilder` 不包含文件读取逻辑 | 本 Plan 完成 | 删除 `from_config_async` |
| 调用点统一 | Plan 4 已完成 | 所有调用点使用 `PromptMaterialLoader` |
| 旧 `PromptConfig` 清理 | 本 Plan 完成 | 标记为 deprecated 或删除 |

### 5.1 删除 `from_config_async`

```diff
 // crates/nova-agent/src/prompt/builder.rs
-use crate::prompt::context::{
-    load_developer_project_prompt_async, load_project_context_with_config_async, EnvironmentSnapshot,
-};
-use crate::prompt::workflow::WorkflowStagePrompts;
+use crate::prompt::context::EnvironmentSnapshot;

 // 删除整个 from_config_async 方法（约 75 行，L262-L336）
-    pub async fn from_config_async(config: &PromptConfig, skills: &SkillRegistry) -> Self {
-        // ... 75 行实现
-    }
```

### 5.2 清理 `PromptConfig` / `PromptLoadContext`

判断 `PromptConfig` 在 Plan 4 完成后是否还有调用者：

- 如果 `prepare_turn` 签名已改为接收 system prompt string（Plan 4 选项 B），则 `PromptConfig` 无调用者，可以删除。
- 如果仍有部分迁移中的 path 使用 `PromptConfig`，则保留并标记 `#[deprecated]`。

**推荐**：在 Plan 4 完成后如果 `PromptConfig` 确认无调用者，直接删除。同时删除 `PromptLoadContext`。

### 5.3 允许保留的 IO

以下 IO 属于其他职责边界，不在本设计清理范围：

| 保留的 IO | 所在模块 | 原因 |
|-----------|---------|------|
| config loader 读取 `.nova/config.toml` | `config/` | 配置加载，非外部资源 discovery |
| builtin read/write/edit 工具访问工作区文件 | `tool/builtin/read.rs` 等 | agent runtime 能力 |
| SQLite conversation store | `conversation/` | 数据持久化 |
| provider HTTP 调用 | `provider/` | LLM 服务调用 |
| 写配置快照或管理任务状态 | `app/` | 状态管理 |
| `PromptMaterialLoader` 中的 IO | `app/prompt_loader.rs` | 外部资源加载层，属于 app 层职责 |

这些 IO 可以留在 `nova-agent` 或当前 crate 中，但不能与 skill/prompt/project/workflow discovery/load 混淆。

### 5.4 验证命令

使用代码搜索验证边界：

```bash
# 1. 确认 skill 核心模块无 loader 引用
rg "nova_skill_loader" crates/nova-agent/src/skill

# 2. 确认 prompt builder 无文件读取
rg "tokio::fs|std::fs|read_to_string" crates/nova-agent/src/prompt/builder.rs

# 3. 确认 bootstrap/service/tool 无直接 prompt 文件读取
rg "load_agent_prompt\b" crates/nova-agent/src/app crates/nova-agent/src/tool
# 预期：只有 PromptMaterialLoader 的引用

# 4. 确认 from_config_async 已删除
rg "from_config_async" crates/nova-agent/src

# 5. 确认 PromptLoadContext 无运行时使用
rg "PromptLoadContext" crates/nova-agent/src
```

第 3 条命令需要人工区分：app 层如果仍承载 `PromptMaterialLoader`，允许 `prompt_loader.rs` 读取文件；不允许 `bootstrap.rs`、`conversation_service.rs`、`agent_workspace_service.rs`、Agent tool、prompt builder 各自重复读取 prompt 文件。

### 5.5 迁移说明

#### Skill 加载迁移

旧方式：

```rust
let mut registry = SkillRegistry::new();
registry.load_from_dir_async(config.skills_dir()).await?;
```

新方式：

```rust
let loaded = nova_skill_loader::load_skills_from_dir_async(config.skills_dir()).await?;
let packages = skill_adapter::convert_loaded_skills(loaded);
let registry = SkillRegistry::from_packages(packages)?;
```

转换函数位于 `app/skill_adapter.rs`，不位于 `nova-agent/src/skill`。

#### Prompt 构建迁移

旧方式：

```rust
let prompt_config = PromptConfig::new(agent_id, agent_prompt, load_context)
    .with_environment(env)
    .with_workflow_prompt_path(...)
    .with_template_vars(vars);
let builder = SystemPromptBuilder::from_config_async(&prompt_config, &skill_registry).await;
```

新方式：

```rust
let prompt_loader = PromptMaterialLoader::from_config(&config);
let material = prompt_loader.load_agent_material(spec, env, catalog, vars).await?;
let turn_material = prompt_loader.load_turn_material(project_dir, stage, skill, turn_vars, enable_dev).await?;
let builder = SystemPromptBuilder::from_material(&material, &turn_material, &skill_registry);
```

#### Agent prompt 加载迁移

旧方式（三个不同实现）：

```rust
// bootstrap.rs
let agent_prompt = load_agent_prompt(agent, &config).await?;

// agent_workspace_service.rs
let prompt_base = load_agent_prompt_for_reload(&agent_spec, &reloaded_config).await?;

// tool/builtin/agent.rs
let prompt = self.load_agent_prompt(spec).await?;
```

新方式（统一实现）：

```rust
let prompt_loader = PromptMaterialLoader::from_config(&config);
let agent_prompt = prompt_loader.load_agent_prompt(spec).await?;
```

## 测试案例

- 正常路径：workspace 全量测试通过（`cargo test --workspace`）。
- 正常路径：`build_application` 能加载 skill、构建 agent registry、注册 builtin tools。
- 正常路径：conversation turn 能通过 material 注入 developer prompt、project context、workflow prompt。
- 边界条件：无 skill、无 developer prompt、无 project context、无 workflow prompt 均可启动。
- 边界条件：默认 agent prompt 文件缺失时保持既有兼容降级。
- 异常场景：显式 `prompt_file` 缺失时所有调用点返回一致错误。
- 异常场景：skill 解析失败时错误不从 `SkillRegistry` 产生，而从 loader/bootstrap 边界产生。
- 验证场景：验证命令（5.4）全部通过。

## 验收标准

- `cargo clippy --workspace -- -D warnings` 通过。
- `cargo fmt --all --check` 通过。
- `cargo test --workspace` 通过。
- `crates/nova-agent/src/skill/` 无 `nova_skill_loader` import。
- `from_config_async` 已从 `builder.rs` 删除。
- `PromptConfig` / `PromptLoadContext` 已删除或标记 deprecated（无运行时调用者）。
- prompt builder 主要构建路径无文件读取。
- bootstrap、conversation_service、agent_workspace_service、Agent tool 不再各自实现 prompt 文件读取 fallback。
- 验证命令（5.4）全部通过。
- docs 中的总览、Plan 1-5 与最终代码状态一致。
