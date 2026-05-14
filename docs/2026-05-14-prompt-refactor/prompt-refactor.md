# Prompt 模块重构总览

## 时间

2026-05-14

## 项目现状

`nova-agent/src/prompt/mod.rs` 是一个 **90KB / 2512 行**的巨型文件，承载了原始实现的全部逻辑。
同时，目录中已存在部分新子模块（`builder.rs`、`context.rs`、`routing.rs`、`types.rs`），
是一次**已启动但未完成**的拆分工作——旧代码并未移除，两套实现并存，外部调用方仍引用旧版。

当前 `mod.rs` 包含的内容：

| 类别 | 主要类型/函数 |
|------|-------------|
| 配置结构 | `SectionName`、`PromptConfig`（15 字段 + 12 builder 方法）、`PromptPriority`、`ProjectInstructionProfile`、`SkillInjectionMode`、`ToolGuidanceMode` |
| 环境采集 | `EnvironmentSnapshot`、`detect_shell_command`、`normalize_shell_command` |
| 文件加载 | `load_project_context`、`load_project_context_with_config`（同步+异步各一对，共 4 函数）；`load_developer_project_prompt`（同步+异步，共 2 函数） |
| Prompt 构建器 | `SystemPromptBuilder`（含 `from_config` 同步+异步）、`build_agent_catalog_section` |
| Turn 上下文 | `TurnContext`、`ActiveSkillState` |
| Skill 路由类型 | `SkillRouteDecision`、`SkillInvocationLevel`、`SkillSwitchResult` |
| 历史裁剪 | `HistoryTrimmer`、`TrimmerConfig`、`TrimResult` |
| 侧信道注入 | `SideChannelInjector`、`SideChannelConfig` |
| 工作流 Prompt | `WorkflowStagePrompts`、`TemplateContext`（模板渲染）、`template_vars` 常量 |
| 测试 | ~800 行测试代码 |

同时存在**同步/异步双写**问题：每个 I/O 函数均维护两份近乎完全一样的实现（sync + async），增加约 30% 额外代码量，且同步版本在 async 上下文中可能阻塞 tokio worker。

## 整体目标

1. **彻底完成已启动的拆分**：将 `mod.rs` 中的旧代码迁移到已存在的新子模块，使新子模块成为 source of truth
2. **消除同步/异步双写**：统一为 async 函数；同步 I/O 只在测试或特殊场景用 `std::fs`，不在库层面维护双版本
3. **使 `mod.rs` 成为纯 re-export 文件**：最终不超过 50 行

## Plan 拆分

| Plan | 说明 | 依赖 | 状态 |
|------|------|------|------|
| Plan 1 | 完成 `mod.rs` → 新子模块迁移，使 `mod.rs` 变为纯 re-export | 无 | 待开始 |
| Plan 2 | 消除同步/异步双写，统一为 async | Plan 1 | 待开始 |

### Plan 1 验收标准（可检查）

1. `nova-agent/src/prompt/mod.rs` 文件长度 <= 50 行。
2. `mod.rs` 仅包含 `mod xxx;` 与 `pub use xxx::...;`，不包含业务实现函数、结构体或测试。
3. 原 `mod.rs` 对外可见类型/函数在新子模块中有唯一定义并可正常导出（编译通过）。
4. 不再保留“旧实现 + 新实现并存”状态：同名核心逻辑在 `prompt` 目录中仅保留一份 source of truth。

### Plan 1 模块保留策略（先定后改）

- `load_project_context*` 系列函数统一以 `context.rs` 为唯一归属；
- `routing.rs` 仅保留 Skill 路由判定相关类型与逻辑，不再承载 project context 加载函数；
- `HistoryTrimmer::trim()` 统一采用 `builder.rs` 当前签名（无 `system_prompt` 参数），并在 Plan 1 完成所有调用方对齐。

## 风险与待定项

- `HistoryTrimmer::trim()` 签名迁移可能影响上游调用链，需要在 Plan 1 中逐一编译校验。
- Plan 1 完成后需立即运行全量检查循环，防止 re-export 漏项在 Plan 2 才暴露。
