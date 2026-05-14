# Plan 3: Prompt Material Loader 与纯 Prompt Builder

## 前置依赖

Plan 1

## 本次目标

将 agent prompt、developer project prompt、project context、workflow prompt 的文件读取逻辑移出 `SystemPromptBuilder` 和分散调用点，建立统一的 `PromptMaterialLoader`。完成后 prompt builder 只消费已加载文本并执行组装策略。

## 当前 IO 分布

| IO 操作 | 当前位置 | 调用方式 |
|---------|---------|---------|
| 读 agent prompt file | `bootstrap.rs:225-262` `load_agent_prompt` | `tokio::fs::read_to_string` |
| 读 agent prompt file | `agent_workspace_service.rs:620-642` `load_agent_prompt_for_reload` | `tokio::fs::read_to_string` |
| 读 agent prompt file | `tool/builtin/agent.rs:377-397` `AgentTool::load_agent_prompt` | `tokio::fs::read_to_string` |
| 读 developer prompt | `prompt/context.rs:286-323` `load_developer_project_prompt_async` | `tokio::fs::read_to_string` |
| 读 project context | `prompt/context.rs:228-271` `load_project_context_with_config_async` | `tokio::fs::read_to_string` |
| 读 workflow prompt | `prompt/workflow.rs:11-13` `WorkflowStagePrompts::load_from_file_async` | `tokio::fs::read_to_string` |
| builder 内 fallback IO | `prompt/builder.rs:290-300` | 调用 `load_developer_project_prompt_async` |
| builder 内 fallback IO | `prompt/builder.rs:304-311` | 调用 `load_project_context_with_config_async` |
| builder 内 fallback IO | `prompt/builder.rs:323-333` | 调用 `WorkflowStagePrompts::load_from_file_async` |

### 重复逻辑对比

三个 `load_agent_prompt*` 函数的优先级逻辑差异：

| 函数 | prompt_file + prompt_inline 同时存在 | legacy template | 默认 agent-{id}.md 不存在 |
|------|-------------------------------------|-----------------|--------------------------|
| `bootstrap::load_agent_prompt` | bail 报错 | warn 后返回 | warn 后返回空 |
| `workspace::load_agent_prompt_for_reload` | bail 报错 | 未处理 | 静默返回空 |
| `AgentTool::load_agent_prompt` | 未处理（prompt_inline 优先） | 静默返回 | 返回固定字符串 `"You are a helpful assistant."` |

目标：统一为一个函数，消除差异。

## 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/nova-agent/src/prompt/types.rs` | 新增类型 | `PromptMaterial`、`TurnPromptMaterial`（Plan 1 已定义） |
| `crates/nova-agent/src/prompt/builder.rs` | 新增方法 | `from_material`（Plan 1 已定义），标记 `from_config_async` 为迁移桥接 |
| `crates/nova-agent/src/prompt/context.rs` | 保留功能 | `load_developer_project_prompt_async`、`load_project_context_with_config_async` 作为 loader 层实现继续存在 |
| `crates/nova-agent/src/prompt/workflow.rs` | 保留功能 | `WorkflowStagePrompts` 作为 loader 层实现继续存在 |
| `crates/nova-agent/src/app/prompt_loader.rs` | 新增 | 统一的 `PromptMaterialLoader` |
| `crates/nova-agent/src/app/bootstrap.rs` | 迁移 | 使用 `PromptMaterialLoader` 替代 `load_agent_prompt` |
| `crates/nova-agent/src/app/conversation_service.rs` | 迁移 | 使用 `PromptMaterialLoader::load_turn_material` |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 迁移 | 使用 `PromptMaterialLoader` 替代 `load_agent_prompt_for_reload` |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 迁移 | 使用 `PromptMaterialLoader` 替代 `AgentTool::load_agent_prompt` |

## 详细设计

### 目标结构

```text
PromptMaterialLoader
    -> resolve agent prompt (prompt_file / prompt_inline / legacy / default)
    -> load developer_project_prompt by project_dir and file list
    -> load project_context by project_dir and configured context path
    -> load workflow_prompt by workflow stage
    -> PromptMaterial / TurnPromptMaterial
    -> SystemPromptBuilder::from_material(...)
```

### PromptMaterialLoader 设计

```rust
// crates/nova-agent/src/app/prompt_loader.rs

use crate::config::{AgentSpec, AppConfig};
use crate::prompt::context::{
    load_developer_project_prompt_async,
    load_project_context_with_config_async,
    EnvironmentSnapshot,
};
use crate::prompt::types::{
    PromptMaterial, TurnPromptMaterial, SkillInjectionMode,
    ProjectInstructionProfile, ToolGuidanceMode,
};
use crate::prompt::workflow::WorkflowStagePrompts;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 统一的 prompt 素材加载器。
///
/// 职责：
/// - 解析 agent prompt 优先级
/// - 加载 developer project prompt、project context、workflow prompt
/// - 记录文件路径类日志
/// - 集中降级策略
///
/// 不负责：
/// - 拼接最终 system prompt section
/// - 调用 skill registry
/// - 计算 prompt diagnostics
pub struct PromptMaterialLoader {
    pub config_dir: PathBuf,
    pub prompts_dir: PathBuf,
    pub project_context_file: Option<PathBuf>,
    pub developer_prompt_files: Vec<String>,
}

impl PromptMaterialLoader {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            config_dir: config.config_dir.clone(),
            prompts_dir: config.prompts_dir(),
            project_context_file: config.project_context_file(),
            developer_prompt_files: config.developer_prompt_files.clone(),
        }
    }

    /// 加载 agent base prompt 内容。
    ///
    /// 优先级：prompt_file > prompt_inline > legacy system_prompt_template > 默认 agent-{id}.md
    /// 约束：prompt_file 和 prompt_inline 不能同时配置。
    pub async fn load_agent_prompt(&self, spec: &AgentSpec) -> Result<String> {
        if spec.prompt_file.is_some() && spec.prompt_inline.is_some() {
            bail!(
                "Agent '{}' has both prompt_file and prompt_inline configured; only one is allowed",
                spec.id
            );
        }

        if let Some(file) = &spec.prompt_file {
            let prompt_path = self.prompts_dir.join(file);
            let content = tokio::fs::read_to_string(&prompt_path)
                .await
                .with_context(|| {
                    format!("Failed to read prompt_file for agent '{}': {:?}", spec.id, prompt_path)
                })?;
            return Ok(content);
        }

        if let Some(inline) = &spec.prompt_inline {
            return Ok(inline.clone());
        }

        if let Some(legacy) = &spec.system_prompt_template {
            log::warn!(
                "Agent '{}' uses legacy system_prompt_template. This field is deprecated; use prompt_file/prompt_inline.",
                spec.id
            );
            return Ok(legacy.clone());
        }

        // 默认文件 fallback
        let default_file = format!("agent-{}.md", spec.id);
        let prompt_path = self.prompts_dir.join(&default_file);
        match tokio::fs::read_to_string(&prompt_path).await {
            Ok(content) => Ok(content),
            Err(err) => {
                log::warn!(
                    "Default prompt file {:?} not found for agent '{}': {}",
                    prompt_path, spec.id, err
                );
                Ok(String::new())
            }
        }
    }

    /// 加载启动期 PromptMaterial。
    pub async fn load_agent_material(
        &self,
        spec: &AgentSpec,
        env: Option<EnvironmentSnapshot>,
        agent_catalog: Option<String>,
        template_vars: HashMap<String, String>,
    ) -> Result<PromptMaterial> {
        let agent_prompt = self.load_agent_prompt(spec).await?;
        Ok(PromptMaterial {
            agent_id: spec.id.clone(),
            agent_prompt,
            agent_catalog,
            environment_snapshot: env,
            initial_template_vars: template_vars,
            skill_injection_mode: SkillInjectionMode::Catalog,
            project_instruction_profile: ProjectInstructionProfile::Auto,
            tool_guidance: ToolGuidanceMode::Compact,
        })
    }

    /// 加载 turn 级动态内容。
    pub async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<TurnPromptMaterial> {
        // Developer project prompt
        let developer_project_prompt = if enable_developer_prompt {
            load_developer_project_prompt_async(
                project_dir,
                &self.developer_prompt_files,
            ).await
        } else {
            None
        };

        // Project context
        let project_context = load_project_context_with_config_async(
            project_dir,
            self.project_context_file.as_deref(),
        ).await;

        // Workflow prompt
        let workflow_prompt = self.load_workflow_prompt(workflow_stage, &turn_vars).await;

        Ok(TurnPromptMaterial {
            developer_project_prompt,
            project_context,
            workflow_prompt,
            turn_template_vars: turn_vars,
            active_skill,
        })
    }

    async fn load_workflow_prompt(
        &self,
        stage: Option<&str>,
        vars: &HashMap<String, String>,
    ) -> Option<String> {
        let stage = stage?;
        if stage == "idle" {
            return None;
        }
        let path = self.prompts_dir.join("workflow-stages.md");
        let prompts = WorkflowStagePrompts::load_from_file_async(&path).await.ok()?;
        prompts.render(stage, vars)
    }
}
```

### SystemPromptBuilder 变更

`from_material` 已在 Plan 1 定义。本 Plan 额外要求：

1. 标记 `from_config_async` 为迁移桥接：
   ```rust
   /// 从 PromptConfig 构建 prompt。
   ///
   /// **迁移桥接**：此方法将在 Plan 4 完成后删除。
   /// 新代码应使用 `from_material`。
   #[deprecated(note = "use from_material instead")]
   pub async fn from_config_async(...) -> Self { ... }
   ```

2. `from_material` 不包含任何 `tokio::fs` / `std::fs` 调用。

### 加载时机

| 场景 | 加载什么 | 调用方 |
|------|---------|-------|
| 启动期 | agent base prompt、agent catalog、environment snapshot、初始 template vars | `bootstrap.rs` via `PromptMaterialLoader::load_agent_material` |
| turn 级 | developer project prompt、project context、workflow prompt、turn template vars | `conversation_service.rs` via `PromptMaterialLoader::load_turn_material` |
| 动态 agent 创建 | agent base prompt（通过 `load_agent_prompt`） | `tool/builtin/agent.rs` via `PromptMaterialLoader` |
| workspace inspect/reload | agent base prompt（通过 `load_agent_prompt`） | `agent_workspace_service.rs` via `PromptMaterialLoader` |

### 缓存策略

第一阶段不强制引入缓存。若后续需要缓存，应位于 loader 层，并以 project dir、文件路径、mtime 或配置版本作为失效依据。`nova-agent` 不感知缓存。

## 迁移步骤

1. 确认 `PromptMaterial` 与 `TurnPromptMaterial` 已定义（Plan 1）。
2. 确认 `SystemPromptBuilder::from_material` 已实现（Plan 1）。
3. 新增 `app/prompt_loader.rs`，实现 `PromptMaterialLoader`。
4. 修改 `app/mod.rs` 增加 `pub(crate) mod prompt_loader;`。
5. 标记 `from_config_async` 为 `#[deprecated]`。
6. 编写 `PromptMaterialLoader` 单元测试（使用临时目录验证 agent prompt 优先级）。
7. 编写 `from_material` 与 `from_config_async` 输出一致性测试。
8. **不在本 Plan 迁移调用点**——调用点迁移在 Plan 4 统一完成。

## 测试案例

- 正常路径：完整 material 生成的 system prompt 与旧 `PromptConfig` 路径生成结果一致。
- 正常路径：`prompt_file`、`prompt_inline`、legacy `system_prompt_template`、默认 `agent-<id>.md` 的优先级保持兼容。
- 正常路径：`load_turn_material` 加载 developer prompt 文件列表，按既有顺序拼接，包含 `### Source:` 分隔。
- 边界条件：developer prompt 文件列表为空时 `developer_project_prompt` 为 `None`。
- 边界条件：project context 文件不存在时 `project_context` 为 `None`。
- 边界条件：workflow stage 为 `"idle"` 时 `workflow_prompt` 为 `None`。
- 边界条件：`enable_developer_prompt` 为 `false` 时不加载 developer prompt。
- 异常场景：`prompt_file` 和 `prompt_inline` 同时配置时报错，错误来自 `PromptMaterialLoader::load_agent_prompt`。
- 异常场景：配置了不存在的显式 `prompt_file` 时返回带 agent id 和路径的错误。

## 验收标准

- `PromptMaterialLoader` 存在且通过测试。
- `from_material` 存在且通过测试。
- `from_config_async` 标记为 `#[deprecated]`。
- prompt builder 的 `from_material` 方法不调用 `tokio::fs` 或 `std::fs`。
- `PromptMaterialLoader` 的三个 `load_agent_prompt` 变体差异已统一为一个实现。
