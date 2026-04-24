# 2026-04-24 tool-skill-capability-enhancement-plan-1

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 1：能力模型与 Skill 包协议统一 |
| 前置依赖 | 无 |
| 本次目标 | 定义稳定的 skill 元数据、capability policy、prompt section 和 session state 协议；兼容现有 `.nova/skills/*/SKILL.md`；明确配置与迁移路径，为后续运行时接入提供统一数据模型。 |
| 涉及文件 | `crates/nova-core/src/skill.rs`、`crates/nova-core/src/config.rs`、`crates/nova-core/src/prompt.rs`、`crates/nova-core/src/agent.rs`、`.nova/README.md`、`.nova/examples/agents.toml`、`docs/2026-04-24-tool-skill-capability-enhancement.md` |

## 详细设计

### 1. skill 数据模型重构

将当前仅包含 `name`、`description`、`body`、`path` 的 `Skill`，升级为可支撑路由和工具裁剪的结构，例如：

- `id`
- `slug`
- `display_name`
- `description`
- `instructions`
- `tool_policy`
- `sticky`
- `aliases`
- `source_path`
- `compat_mode`

兼容策略：

1. 若目录下只有 `SKILL.md`，按兼容模式加载：
   - `slug` 默认使用相对路径；
   - `display_name` 默认取目录名或 frontmatter 的 `name`；
   - `instructions` 取 markdown 正文；
   - `tool_policy` 默认 `inherit_all`；
   - `sticky` 默认 `false`。
2. 若存在 `skill.toml`，则以结构化字段为准，`SKILL.md` 仅作说明文档或 fallback。

### 2. SkillRegistry 的职责收敛

当前 `SkillRegistry` 只负责读一层目录并拼 prompt。改造后职责拆为：

1. 递归扫描 skill 根目录；
2. 解析技能包并生成 `SkillPackage` 列表；
3. 提供按 `id` / `slug` / `alias` 查询；
4. 提供供路由器使用的候选清单；
5. 不再负责直接生成整包 system prompt。

这样做的原因：

- prompt 组装是 turn 级逻辑，不该耦合在 registry；
- registry 应该是“静态定义层”，而不是“运行时渲染层”。

### 3. CapabilityPolicy 模型

新增统一策略对象，用于描述当前轮次可见能力：

- `always_enabled_tools`
- `deferred_tools`
- `tool_search_enabled`
- `skill_tool_enabled`
- `task_tools_enabled`
- `agent_tools_enabled`

策略来源按优先级合成：

1. 运行入口默认策略；
2. 当前 agent 规格；
3. active skill 的 `tool_policy`；
4. 用户显式模式切换或后续交互状态。

这个模型会成为后续 Plan 2 和 Plan 3 的公共基础，避免各处通过 `tool_whitelist: Option<Vec<String>>` 做零散判断。

### 4. PromptSectionBuilder 抽象

`SystemPromptBuilder` 从简单字符串拼接改为 section-based builder：

- `base_section`
- `agent_section`
- `skill_section`
- `environment_section`
- `workflow_section`
- `tool_guidance_section`

每个 section 单独构造，最后统一 `build()`。同时保留一个调试接口返回 section 列表，便于 CLI 输出和单测断言。

### 5. 配置结构扩展

在 `config.rs` 中增加 skill / capability 相关配置，但先不增加新依赖：

- `tool.skills_dir`
- `tool.default_policy`
- `gateway.skill_routing_enabled`
- `gateway.skill_history_strategy`

约束：

- 必须给出默认值，保证旧配置可直接运行；
- 优先使用字符串枚举和简单结构，避免一次性引入复杂嵌套。

## 测试案例

1. 正常路径：从 `.nova/skills/skill-creator/SKILL.md` 成功加载 skill，生成稳定的 `slug`、`display_name`、`instructions`。
2. 正常路径：递归扫描嵌套目录，确保不仅限于一级子目录。
3. 边界条件：`SKILL.md` 无 frontmatter 时，仍能按目录名生成兼容 skill。
4. 边界条件：同名 `slug` 或 alias 冲突时，返回带上下文的错误。
5. 异常场景：`skill.toml` 存在但格式非法，加载失败且错误信息指向具体文件。
6. 正常路径：Prompt builder 能按 section 顺序输出，不因某 section 缺失而产生重复空白。
7. 异常场景：旧配置文件缺少新字段时，`AppConfig` 仍能成功加载并落到默认策略。
