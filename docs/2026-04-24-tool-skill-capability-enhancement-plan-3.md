# 2026-04-24 tool-skill-capability-enhancement-plan-3

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 3：Tool 暴露策略、Task 编排与 ToolSearch 协同 |
| 前置依赖 | Plan 1、Plan 2 |
| 本次目标 | 把当前零散存在的 loaded/deferred tool、task store、skill tool、agent tool 串成统一的能力暴露策略，减少无关工具噪音，并让复杂 skill 能基于 Task 与 ToolSearch 形成稳定工作流。 |
| 涉及文件 | `crates/nova-core/src/tool.rs`、`crates/nova-core/src/tool/builtin/mod.rs`、`crates/nova-core/src/tool/builtin/tool_search.rs`、`crates/nova-core/src/tool/builtin/task.rs`、`crates/nova-core/src/tool/builtin/skill.rs`、`crates/nova-core/src/tool/builtin/agent.rs`、`crates/nova-core/src/event.rs`、`crates/nova-core/src/prompt.rs` |

## 详细设计

### 1. ToolRegistry 与 CapabilityPolicy 对接

当前 `register_builtin_tools(..., tool_whitelist)` 仍是静态注册思路。目标上改成：

1. 启动时注册全部 builtin tool，其中一部分为 deferred；
2. 每轮根据 `CapabilityPolicy` 生成“当前暴露视图”；
3. provider 看到的是视图，而不是整个 registry。

视图包含：

- `loaded_for_turn`
- `deferred_for_turn`
- `tool_search_enabled`

这样就不需要为不同入口重复构造多个 registry，也便于 skill 切换时动态调整。

### 2. ToolSearch 的职责强化

`ToolSearch` 需要从“按名字 select”增强为“能力发现入口”：

- 支持按关键字搜索 deferred tool；
- 支持按类别过滤，如 `task`、`skill`、`search`；
- 返回结果不仅含 schema，还要有“为什么推荐这个工具”的简述；
- 成功加载后发出 `ToolUnlocked` 事件。

同时保留 `select:Name` 快速路径，以兼容当前实现。

### 3. SkillTool 的定位调整

当前 `SkillTool` 更像“读取 skill 文本”。在新设计下，它应当是补充机制，而不是主激活机制：

1. 主路径：session 通过路由器激活 active skill；
2. 补充路径：模型在当前 skill 内想加载外部专用说明时，可调用 `SkillTool`；
3. `SkillTool` 输出需带结构化元数据，而不是只返回一段拼接文本。

这样可以避免“skill 系统”和“Skill tool”争夺主流程控制权。

### 4. Task 工具与工作流编排

Task 工具默认只在以下场景暴露：

- active skill 标记为 workflow / planner；
- 当前 agent 类型允许多步骤计划；
- 用户明确请求“拆分任务/给出 plan/按步骤执行”。

编排约束：

1. 一个 session 内同一时刻最多一个 `in_progress` 主任务；
2. 子任务可挂在 `metadata.parent_id` 下；
3. 若 skill 为 sticky workflow，则任务状态与 skill 生命周期关联。

### 5. Agent 工具与 skill/工具策略联动

当前 `Agent` 工具已存在，但其子代理使用的工具集需要跟 capability policy 对齐：

- 默认继承当前 skill 的工具策略；
- 若 agent spec 自带更窄的 whitelist，则进一步收缩；
- 子代理不自动继承父级 active skill，除非显式指定。

这可以避免父会话中的高权限 skill 或大工具集泄漏给子代理。

## 测试案例

1. 正常路径：默认对话只看到基础工具与 `ToolSearch`，不会一次性暴露全部 deferred tool。
2. 正常路径：调用 `ToolSearch` 后，指定 deferred tool 被加载并出现在后续轮次工具定义中。
3. 正常路径：workflow skill 激活后，Task 工具可见并能正常创建、更新、列出任务。
4. 边界条件：普通闲聊 skill 下不暴露 Task 工具，模型无法误创建任务。
5. 边界条件：`SkillTool` 在 active skill 已存在时仍可作为补充说明加载，但不能覆盖 active skill 状态。
6. 异常场景：请求不存在的 deferred tool 时，`ToolSearch` 返回可诊断错误并列出接近候选。
7. 异常场景：子代理工具策略计算失败时，回退到最小权限集合，而不是继承全部工具。
