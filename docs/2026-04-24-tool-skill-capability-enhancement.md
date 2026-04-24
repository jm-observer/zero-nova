# 2026-04-24 tool-skill-capability-enhancement

| 章节 | 说明 |
|------|------|
| 时间 | 创建：2026-04-24；最后更新：2026-04-24 |
| 项目现状 | `nova-core` 已具备 `Edit`、`Skill`、`Task*`、`ToolSearch`、`Agent` 等原型能力，并支持 `.nova/skills`、延迟工具注册、任务事件和基础 prompt 构建；但 skill 仍以“扫描 `SKILL.md` 后整包注入 system prompt”为主，缺少按需路由、生命周期管理、工具裁剪、评测闭环和前后端一致的能力暴露。 |
| 整体目标 | 在不新增外部依赖、尽量复用现有 `nova-core`/`nova-app`/`nova-cli` 架构的前提下，把当前分散的 tool 与 skill 原型收敛为一套可配置、可观测、可逐步扩展的能力系统，使 Zero-Nova 能稳定支持“按需暴露工具 + 按需激活技能 + 多轮工作流编排 + 能力评测与回归验证”。 |
| Plan 拆分 | Plan 1：统一能力模型与 Skill 包结构，先定义“系统里什么是 skill、什么是 tool policy、如何加载”。<br>Plan 2：实现 skill 路由、激活态和 prompt 组装，解决“何时启用哪个 skill、上下文如何保留”。<br>Plan 3：重构 tool 暴露与调用策略，解决“当前轮次向模型暴露哪些工具、任务和 ToolSearch 怎么协同”。<br>Plan 4：补齐 CLI / gateway / deskapp 观测、配置、评测和测试，使能力系统能被真实使用并可持续演进。 |
| 风险与待定项 | 1. 现有 SkillTool 是“把指令作为工具返回”，与“系统级激活 skill”是两条机制，需避免重复或冲突。<br>2. 当前 `SkillRegistry` 解析 `SKILL.md` 的方式较弱，兼容现有 skill 包时要避免一次性破坏。<br>3. tool 延迟加载已存在，但当前 prompt 仍偏向一次性暴露，需要重新定义和 provider 的交互策略。<br>4. 若要做 LLM 路由器，需复用现有主模型配置或已有路由提示模板，避免引入新的复杂配置面。 |

## 项目现状

结合当前仓库代码，tool / skill 相关能力处于“原型已散落落地，但没有闭环集成”的状态：

1. `crates/nova-core/src/tool/` 已经具备较完整的工具框架：
   - `ToolRegistry` 支持 loaded / deferred 两类工具；
   - `ToolSearch` 已能按名称加载 deferred tool；
   - `TaskCreate`、`TaskList`、`TaskUpdate` 已存在并向事件系统发进度事件；
   - `Read` / `Write` / `Edit` 已共享 `read_files` 状态；
   - `Agent` 子代理工具已存在。
2. `SkillRegistry` 已能从 `.nova/skills` 目录加载 `SKILL.md`，但仅支持一层目录扫描，解析规则简单，且最终只是把所有 skill 内容直接拼进统一 system prompt。
3. `nova-cli`、`nova-app` 已在启动时加载 skill registry 和 task store，但并没有围绕“active skill”“tool subset”“skill sticky”“历史压缩”等概念建立会话状态机。
4. `SystemPromptBuilder` 仍偏静态拼接器，缺少 base prompt、active skill prompt、环境信息、工具概览、运行时状态等分层模型。
5. 已有历史文档分别讨论过 skill system、tool enhancement、Claude Code 使用方式，但没有一份文档基于“当前代码已经实现了什么”来重新整理实施路径。

这意味着本次设计不能简单重复“从零引入 Skill/Task/ToolSearch”，而应该处理以下真正缺口：

- skill 定义与加载协议不稳定；
- skill 激活机制没有进入主运行时；
- tool 暴露策略和 skill / agent / task 没串起来；
- 会话层缺少可视化、可回归、可测试的能力闭环。

## 整体设计

### 1. 设计原则

1. 先收敛模型，再补实现：先统一 skill / tool / capability policy 的数据结构与边界，避免继续堆叠点状特性。
2. 保持兼容：保留对现有 `.nova/skills/*/SKILL.md` 的兼容读取，逐步过渡到更强的 skill 包结构。
3. 优先复用现有原型：沿用 `ToolRegistry`、`TaskStore`、`AgentEvent`、`config.skills_dir()`、`SystemPromptBuilder`，不重造平行系统。
4. 运行时只暴露当前轮次需要的能力：避免继续把所有 skill 内容和所有 tool schema 都塞进每轮 prompt。
5. 让能力系统可观测：无论是 skill 激活、tool search、tool 延迟加载还是 task 编排，都必须在 CLI / gateway / UI 层有清晰事件。

### 2. 目标能力模型

目标上引入三个清晰层级：

1. `SkillPackage`
   - 描述一个可被路由和激活的技能包；
   - 包含标识、描述、说明文档、工具策略、可选 sticky 行为、可选参数模板。
2. `CapabilityPolicy`
   - 描述某个会话或某一轮真正允许暴露给模型的工具集合；
   - 由 active skill、当前 agent 类型、运行模式（CLI / gateway / desktop）共同决定。
3. `TurnContext`
   - 描述当前轮次的系统提示词、active skill、工具定义、历史裁剪结果、可见状态摘要；
   - 作为 `AgentRuntime::run_turn` 之前的显式准备步骤。

三者关系如下：

- `SkillPackage` 负责“语义与行为”；
- `CapabilityPolicy` 负责“权限与暴露”；
- `TurnContext` 负责“真正送给模型的输入”。

### 3. Skill 系统目标形态

#### 3.1 skill 包结构

兼容两种格式，但统一抽象到同一数据模型：

1. 兼容格式：`<skill>/SKILL.md`
   - 继续支持现有 skill-creator 风格的目录；
   - frontmatter 或目录名提供最小元信息。
2. 目标格式：

```text
.nova/skills/<slug>/
├── SKILL.md
├── skill.toml
├── prompts/
│   ├── system.md
│   └── router.md
├── scripts/
└── assets/
```

其中：

- `SKILL.md` 保留人类可读说明与兼容入口；
- `skill.toml` 提供结构化元数据；
- `prompts/system.md` 是 skill 激活时注入的系统片段；
- `prompts/router.md` 可选，用于提升该 skill 被路由命中的判别质量；
- `scripts/`、`assets/` 供 skill 内部说明引用，但不自动执行。

#### 3.2 skill 元数据

建议统一为以下字段：

- `id` / `slug`
- `display_name`
- `description`
- `entry_prompt`
- `tool_policy`
- `sticky`
- `aliases`
- `examples`
- `source_path`

其中 `tool_policy` 不直接写成“工具白名单字符串数组”这么简单，而要支持三类模式：

- `inherit_all`
- `allow_list`
- `allow_list_with_deferred`

这样可以覆盖：

- 普通 skill 只开放少量常驻工具；
- 高复杂度 skill 依赖 `ToolSearch` 再按需补充；
- 默认对话不激活任何 skill 时继续使用系统默认工具集。

#### 3.3 skill 生命周期

每个 session 维护一个显式 `ActiveSkillState`：

- `inactive`
- `candidate`
- `active`
- `exiting`

生命周期规则：

1. 用户消息进入后，先根据 active state 决定是否需要路由。
2. 若当前 skill 为 sticky，默认跳过重路由，继续沿用当前 skill。
3. 若切换 skill，旧 skill 的会话摘要写入 skill history segment，新 skill 进入 active。
4. 若模型或用户触发退出标记，active skill 退回 inactive。

这样既保留旧文档里的 sticky 思路，也避免当前“SkillTool 被当成普通工具调用后立即失效”的问题。

### 4. Tool 系统目标形态

#### 4.1 工具暴露分层

工具分为四组：

1. 常驻基础工具
   - `Bash`、`Read`、`Write`、`Edit`
2. 检索工具
   - `WebSearch`、`WebFetch`
3. 编排工具
   - `TaskCreate`、`TaskList`、`TaskUpdate`、`Agent`
4. 发现工具
   - `ToolSearch`、`Skill`

运行时不再默认把第 3、4 组全部暴露给模型，而是由 `CapabilityPolicy` 选择：

- 默认对话：基础工具 + 必要检索工具；
- 复杂工作流 skill：加入任务编排工具；
- 需要扩展能力时：暴露 `ToolSearch`；
- 需要显式载入外部 skill 指令时：暴露 `Skill`。

#### 4.2 deferred tool 的真实职责

当前 deferred tool 只是实现了“可延迟注册”，但还未进入完整工作流。目标上：

1. provider 初始只看见当前轮允许的 loaded tool + 一个 `ToolSearch`。
2. 模型判断当前工具不足时，通过 `ToolSearch` 请求 schema。
3. registry 将 deferred tool 提升为 loaded tool，并在后续迭代中可见。
4. event 层记录“哪个工具因何被解锁”，供 CLI / UI 展示。

这样才能真正实现 Claude Code 风格的“能力逐步暴露”，避免 system prompt 和 tool schema 一次性膨胀。

#### 4.3 Task 工具的定位

Task 不只是“可选工具”，而应该成为复杂 skill 的标准编排层：

- 长流程 skill 默认启用 `TaskCreate/List/Update`；
- CLI / gateway / deskapp 都消费任务事件并展示进度；
- 后续可为计划型 skill 提供默认 task 模板。

### 5. Prompt 与会话上下文

#### 5.1 system prompt 分层

目标 system prompt 由以下片段构成：

1. base prompt
2. 当前 agent prompt
3. active skill prompt
4. workflow / pending interaction prompt
5. environment snapshot
6. tool usage guidance

`SystemPromptBuilder` 需要从“字符串累加器”升级为“具名 section builder”，以便：

- 做条件注入；
- 控制顺序；
- 在 CLI 中调试输出；
- 测试时可断言具体 section 是否存在。

#### 5.2 历史管理

skill 切换后不保留整段原始消息，而是切成 segments：

- global segment
- per-skill segment
- current active segment

第一阶段先用规则摘要：

- 保留用户目标；
- 保留关键决策；
- 保留未完成事项；
- 丢弃冗长工具日志。

后续若已有合适的内部 LLM 路由能力，再扩展成 LLM 摘要。

### 6. 观测与评测

能力系统要可持续迭代，必须从一开始补上两类闭环：

1. 运行时观测
   - skill loaded
   - skill activated / exited / switched
   - tool unlocked by ToolSearch
   - capability policy changed
   - task state changed
2. 回归评测
   - skill discovery 成功率
   - skill routing 稳定性
   - tool selection 正确率
   - 长流程任务中 task 生成与状态演进是否符合预期

评测资产优先落在 `.nova/examples/` 与 `docs/` 下，复用当前已有 workflow 示例，而不是引入额外框架。

## Plan 拆分

### Plan 1：能力模型与 Skill 包协议统一

- 文件：`docs/2026-04-24-tool-skill-capability-enhancement-plan-1.md`
- 目标：把 skill / capability / prompt section 的核心数据模型先定义稳定，明确兼容策略和配置结构。
- 依赖：无

### Plan 2：Skill 路由、激活态与 Prompt 组装

- 文件：`docs/2026-04-24-tool-skill-capability-enhancement-plan-2.md`
- 目标：让 skill 真正进入会话主流程，解决 active skill、sticky、历史切片、prompt 动态组装。
- 依赖：Plan 1

### Plan 3：Tool 暴露策略、Task 编排与 ToolSearch 协同

- 文件：`docs/2026-04-24-tool-skill-capability-enhancement-plan-3.md`
- 目标：把 deferred tool、task tool、agent tool 和 skill tool 串成统一 capability policy，减少 prompt/tool 噪音。
- 依赖：Plan 1、Plan 2

### Plan 4：CLI / Gateway / DeskApp 集成、观测与评测

- 文件：`docs/2026-04-24-tool-skill-capability-enhancement-plan-4.md`
- 目标：把前 3 个 plan 落到真实入口、事件协议、配置和测试里，形成可交付闭环。
- 依赖：Plan 2、Plan 3

## 执行顺序建议

1. 先做 Plan 1，先把协议和状态模型定住。
2. 再做 Plan 2，让 skill 生命周期进入主链路。
3. 然后做 Plan 3，收敛 tool 暴露与任务编排。
4. 最后做 Plan 4，把 CLI / gateway / desktop 和评测打通。

这个顺序的原因是：当前仓库最大的问题不是“缺少某个单独功能”，而是“原型存在但没有统一收口”。如果先做 UI 或单个 tool 增强，只会继续放大结构债务。
