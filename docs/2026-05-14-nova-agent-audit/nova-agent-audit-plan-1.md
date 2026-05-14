# Plan 1：🔴 严重问题——超规模文件

## Plan 编号与标题

Plan 1：严重问题（超规模文件）

## 前置依赖

无

## 本次目标

识别并记录当前超出 500 行规范上限的巨型文件，明确其职责边界混乱的具体表现。

## 涉及文件

- `src/prompt/mod.rs`
- `src/conversation/service/mod.rs`
- `src/conversation/repository/mod.rs`

---

## 问题 1：`prompt/mod.rs` — 90 KB / 2512 行

这是 crate 中最严重的问题。单个 `mod.rs` 把毫无关联的功能全部堆叠在一起：

| 职责 | 包含内容 |
|------|---------|
| 配置结构 | `SectionName`、`PromptConfig`（15 字段 + 12 builder）、`PromptPriority`、`ProjectInstructionProfile`、`SkillInjectionMode`、`ToolGuidanceMode` |
| 环境采集 | `EnvironmentSnapshot`、`detect_shell_command`、`normalize_shell_command` |
| 文件加载 | `load_project_context`（同步+异步共 4 函数）、`load_developer_project_prompt`（同步+异步共 2 函数） |
| Prompt 构建器 | `SystemPromptBuilder`（`from_config` 同步+异步）、`build_agent_catalog_section` |
| Turn 上下文 | `TurnContext`、`ActiveSkillState` |
| Skill 路由类型 | `SkillRouteDecision`、`SkillInvocationLevel`、`SkillSwitchResult` |
| 历史裁剪 | `HistoryTrimmer`、`TrimmerConfig`、`TrimResult` |
| 侧信道注入 | `SideChannelInjector`、`SideChannelConfig` |
| 工作流 Prompt | `WorkflowStagePrompts`、`TemplateContext`、`template_vars` 常量 |
| 测试 | ~800 行 |

**额外情况**：目录中已存在 `builder.rs`、`context.rs`、`routing.rs`、`types.rs` 等新子模块，是一次**已启动但未完成**的拆分工作——旧代码并未移除，两套实现并存，外部调用方仍引用旧版的 `mod.rs`。

**建议拆分目标：**

```
prompt/
├── mod.rs           # 仅 re-export，≤ 60 行
├── builder.rs       # SystemPromptBuilder（已存在）
├── config.rs        # PromptConfig（可合入 types.rs）
├── context.rs       # EnvironmentSnapshot + shell 检测（已存在）
├── side_channel.rs  # SideChannelInjector（新建）
├── templates.rs     # 常量 + TemplateContext（已存在）
├── trimmer.rs       # HistoryTrimmer（新建）
├── types.rs         # 纯数据结构（已存在）
└── workflow.rs      # WorkflowStagePrompts（新建）
```

对应的详细重构计划见：[`../2026-05-14-prompt-refactor/prompt-refactor.md`](../2026-05-14-prompt-refactor/prompt-refactor.md)

---

## 问题 2：`conversation/service/mod.rs` — 50 KB

`SessionService` 主实现集中在单个 `mod.rs`。虽已拆出 `write.rs`、`queries.rs`、`title.rs` 等子文件，但核心路由逻辑仍留在 `mod.rs` 中，文件过大。

**建议**：将 `SessionService` 的核心调度逻辑拆入 `dispatch.rs` 或按操作类型进一步分拆。

---

## 问题 3：`conversation/repository/mod.rs` — 38 KB

`SqliteSessionRepository` 的全部 CRUD 实现在单个文件，纯 SQL 操作。

**建议**：按 Session / Message 维度拆分为 `session_repo.rs`、`message_repo.rs`（目前这两个文件是空壳 re-export，实现从未迁入）。
