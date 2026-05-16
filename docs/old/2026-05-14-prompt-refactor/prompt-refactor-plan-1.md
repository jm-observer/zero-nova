# Plan 1: 完成 prompt/mod.rs 拆分，使 mod.rs 成为纯 re-export

## Plan 编号与标题

Plan 1: 完成 `mod.rs` → 新子模块迁移

## 前置依赖

无

## 本次目标

- `prompt/mod.rs` 最终只保留子模块声明和 `pub use` re-export，不超过 60 行
- 旧版实现完全迁移到对应子模块（利用已存在的 `builder.rs`、`context.rs`、`types.rs`，新增 `side_channel.rs`、`trimmer.rs`、`workflow.rs`）
- 所有外部调用方（`use crate::prompt::XXX`）无需修改（通过 re-export 保持兼容）
- `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo test` 全部通过

## 涉及文件

| 文件 | 操作 |
|------|------|
| `src/prompt/mod.rs` | 替换为纯 re-export（~50 行） |
| `src/prompt/types.rs` | 已存在，补充 `HistoryTrimmer`、`TrimResult`、`TrimmerConfig` 等缺失类型（或单独拆文件） |
| `src/prompt/builder.rs` | 已存在，确认 `SystemPromptBuilder` 已完整，与 mod.rs 旧版对齐 |
| `src/prompt/context.rs` | 已存在，确认包含 `EnvironmentSnapshot`、shell 检测、项目上下文加载 |
| `src/prompt/trimmer.rs` | 新建，迁移 `HistoryTrimmer`、`TrimmerConfig`、`TrimResult` |
| `src/prompt/side_channel.rs` | 新建，迁移 `SideChannelInjector`、`SideChannelConfig` |
| `src/prompt/workflow.rs` | 新建，迁移 `WorkflowStagePrompts` |

## 详细设计

### 迁移映射

```
mod.rs 旧代码                →  目标子模块
--------------------------------------------------
SectionName                  →  types.rs（已存在）
PromptPriority               →  types.rs（已存在）
NamedSection                 →  types.rs（已存在）
PromptSectionSize / ToolSize →  types.rs（已存在）
PromptConfig + impl          →  types.rs（已存在）
TurnContext / ActiveSkillState→  types.rs（已存在）
SkillRouteDecision 等        →  types.rs（已存在）
AgentCatalogEntry            →  types.rs（已存在）
EnvironmentSnapshot          →  context.rs（已存在）
detect_shell_command 等      →  context.rs（已存在）
load_project_context_*       →  context.rs（已存在，注意重名冲突）
load_developer_project_prompt_*→ context.rs（已存在）
TrimmerConfig / HistoryTrimmer →  trimmer.rs（新建）
SideChannelConfig/Injector   →  side_channel.rs（新建）
WorkflowStagePrompts         →  workflow.rs（新建）
SystemPromptBuilder 等       →  builder.rs（已存在，需补全）
filter_project_instruction_* →  builder.rs 或 types.rs
BEHAVIOR_GUARDS              →  templates.rs（已存在）
TemplateContext / template_vars→ templates.rs（已存在）
```

### 重名函数处理

`routing.rs` 中有 `load_project_context(dir, configured_path)` — 两参数版本；
`context.rs` 中有 `load_project_context(dir)` / `load_project_context_with_config(dir, configured_path)` — 单参数版本。

决策：**以 `context.rs` 为准**（函数名与 mod.rs 旧版保持一致），删除 `routing.rs` 中的重复实现，
同时将 `routing.rs` 常量（`MAX_PROJECT_CONTEXT_CHARS`、`PROJECT_CONTEXT_FILES`）移入 `templates.rs`（已有）。

### 最终 mod.rs 结构

```rust
pub mod builder;
pub mod context;
pub mod side_channel;
pub mod templates;
pub mod trimmer;
pub mod types;
pub mod workflow;
mod routing; // 已清空后删除，或改为 pub(crate) 内部用

// re-export
pub use builder::{SystemPromptBuilder, TrimmerConfig, SideChannelConfig, ...};
pub use context::{EnvironmentSnapshot, load_project_context, ...};
pub use templates::{BEHAVIOR_GUARDS, TemplateContext, template_vars, ...};
pub use types::{PromptConfig, SectionName, TurnContext, ...};
pub use trimmer::{HistoryTrimmer, TrimResult};
pub use side_channel::SideChannelInjector;
pub use workflow::WorkflowStagePrompts;
```

## 测试案例

- 全部现有 `mod.rs` 测试迁移到对应子模块的 `#[cfg(test)]` 块
- 保持测试函数名和测试内容不变
- `cargo test -p nova-agent` 全部通过
