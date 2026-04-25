# 2026-04-24 tool-skill-capability-enhancement

| 章节 | 说明 |
|------|------|
| 时间 | 创建：2026-04-24；最后更新：2026-04-24 |
| 项目现状 | `nova-core` 已具备 `Bash`、`Read`、`Write`、`Edit`、`Skill`、`Task*`、`ToolSearch`、`Agent` 等原型能力，并支持 `.nova/skills`、延迟工具注册、任务事件和基础 prompt 构建；但 skill 仍以"扫描 `SKILL.md` 后整包注入 system prompt"为主，缺少按需路由、生命周期管理、工具裁剪、评测闭环和前后端一致的能力暴露。 |
| 整体目标 | 在不新增外部依赖、尽量复用现有 `nova-core`/`nova-app`/`nova-cli` 架构的前提下，把当前分散的 tool 与 skill 原型收敛为一套可配置、可观测、可逐步扩展的能力系统，使 Zero-Nova 能稳定支持"按需暴露工具 + 按需激活技能 + 多轮工作流编排 + 能力评测与回归验证"。 |
| Plan 拆分 | Plan 1：统一能力模型与 Skill 包结构，先定义"系统里什么是 skill、什么是 tool policy、如何加载"。<br>Plan 2：实现 skill 路由、激活态和 prompt 组装，解决"何时启用哪个 skill、上下文如何保留"。<br>Plan 3：重构 tool 暴露与调用策略，解决"当前轮次向模型暴露哪些工具、任务和 ToolSearch 怎么协同"。<br>Plan 4：补齐 CLI / gateway / deskapp 观测、配置、评测和测试，使能力系统能被真实使用并可持续演进。 |
| 风险与待定项 | <ol><li>现有 `SkillTool` 是"把指令作为工具返回"，与"系统级激活 skill"是两条机制，需避免重复或冲突。</li><li>当前 `SkillRegistry` 解析 `SKILL.md` 的方式较弱（单层目录、简单文本分割），兼容现有 skill 包时要避免一次性破坏。</li><li>tool 延迟加载已存在（`ToolRegistry.deferred`），但当前 prompt 仍偏向一次性暴露，需要重新定义和 provider 的交互策略。</li><li>若要做 LLM 路由器，需复用现有主模型配置（如 `config.rs` 中的 `provides` 链），避免引入新的复杂配置面。</li><li>**新增风险**：`AgentRuntime` 目前可能跨会话复用，`ActiveSkillState` 必须放在 `nova-conversation` session 层，否则会泄漏状态。</li><li>**新增风险**：当前 `tool_whitelist` 是静态注册（启动时决定哪些工具进入 registry），Plan 3 的目标是将"注册"与"暴露"解耦，需要对 `run_turn` 中的 tool 视图做适配。**</li></ol> |

## 项目现状

结合当前仓库代码，tool / skill 相关能力处于"原型已散落落地，但没有闭环集成"的状态：

### 1. Tool 框架（`nova-core/src/tool.rs` 已验证）

- `ToolRegistry` 支持 loaded + deferred 两类工具（line 51-54）
- `ToolSearch` 已能按 `select:Name` 模式加载 deferred tool（`tool/builtin/tool_search.rs`）
- `TaskCreate`、`TaskList`、`TaskUpdate` 已存在并向事件系统发进度事件（通过 `ToolContext.task_store`）
- `Read` / `Write` / `Edit` 已共享 `read_files` 状态（`ToolContext.read_files: Arc<MutexHashSet<String>>`）
- `Agent` 子代理工具已存在，使用 `tool_whitelist` 限制子代理可见工具
- **注意**：当前 `ToolRegistry.execute()` 已有 legacy 名称映射（`bash` → `Bash` 等），详见 line 189-196

### 2. Skill 系统（`nova-core/src/skill.rs` 已验证）

- `SkillRegistry` 从 `.nova/skills` 目录加载 `SKILL.md`
- 仅支持单层目录扫描（非递归）
- 解析规则：使用 `---` 分割 content，匹配 `name:` / `description:` 行
- 最终把所有 skill 内容直接拼进统一 system prompt（`generate_system_prompt()`）
- `ToolContext` 已有 `skill_registry: Option<Arc<SkillRegistry>>` 字段

### 3. 运行时状态

- `nova-cli`、`nova-app` 在启动时加载 skill registry 和 task store
- 没有围绕"active skill"、"tool subset"、"skill sticky"、"历史压缩"等概念建立会话状态机
- `AgentRuntime::run_turn()` 是唯一入口，接受 `history: &[Message]` + `user_input`，返回 `TurnResult`

### 4. Prompt 系统（`nova-core/src/prompt.rs` 已验证）

- `SystemPromptBuilder` 仍偏静态字符串累加器
- 有 `sections: Vec<String>` 雏形，但缺少命名 section、条件注入、section 级调试接口
- 当前方法：`role()`、`guideline()`、`environment()`、`custom_instruction()`、`extra_section()`

---

### 5. 关键缺口总结

这意味着本次设计不能简单重复"从零引入 Skill/Task/ToolSearch"，而应该处理以下真正缺口：

| 缺口 | 当前状态 | 目标状态 |
|------|----------|----------|
| skill 定义与加载协议 | `SKILL.md` 纯文本，单层扫描 | 结构化 `skill.toml` + 递归扫描 |
| skill 激活机制 | 静默注入 system prompt | 显式 `TurnContext` 驱动 |
| tool 暴露策略 | 静态 `tool_whitelist` 注册 | 动态 capability policy |
| ToolSearch | 仅支持 `select:Name` | 支持分类检索 + schema 返回 |
| Task 编排 | 消息级事件流 | 会话级生命周期管理 |
| 历史管理 | 全量消息平铺 | segment 结构 + 规则摘要 |
| 可观测性 | `log!` + `println!` 混合 | `AgentEvent` 标准化事件流 |

---

## 整体设计

### 1. 设计原则

1. **先收敛模型，再补实现**：先统一 skill / tool / capability policy 的数据结构与边界，避免继续堆叠点状特性。
2. **保持兼容**：保留对现有 `.nova/skills/*/SKILL.md` 的兼容读取，逐步过渡到更强的 skill 包结构。
3. **优先复用现有原型**：沿用 `ToolRegistry`、`TaskStore`、`AgentEvent`、`config.skills_dir()`、`SystemPromptBuilder`，不重造平行系统。
4. **运行时只暴露当前轮次需要的能力**：避免继续把所有 skill 内容和所有 tool schema 都塞进每轮 prompt。
5. **让能力系统可观测**：无论是 skill 激活、tool search、tool 延迟加载还是 task 编排，都必须在 CLI / gateway / UI 层有清晰事件。
6. **状态所有权清晰**：session 数据（active skill history）属于 `nova-conversation` 层，运行时不可变的数据（skill definitions）属于 `nova-core` 层。

### 2. 目标能力模型

目标上引入三个清晰层级：

1. **`SkillPackage`**（语义与行为）
   - 描述一个可被路由和激活的技能包；
   - 包含标识、描述、说明文档、工具策略、可选 sticky 行为、可选参数模板。

2. **`CapabilityPolicy`**（权限与暴露）
   - 描述某个会话或某一轮真正允许暴露给模型的工具集合；
   - 由 active skill、当前 agent 类型、运行模式（CLI / gateway / desktop）共同决定。

3. **`TurnContext`**（模型输入）
   - 描述当前轮次的系统提示词、active skill、工具定义、历史裁剪结果、可见状态摘要；
   - 作为 `AgentRuntime::run_turn` 之前的显式准备步骤。

三者关系：

```
SkillPackage ──► CapabilityPolicy ──► TurnContext
  (定义)           (过滤/选择)          (构造模型输入)
```

- `SkillPackage` 负责"语义与行为"；
- `CapabilityPolicy` 负责"权限与暴露"；
- `TurnContext` 负责"真正送给模型的输入"。

---

### 3. Skill 系统目标形态

#### 3.1 skill 包结构

兼容两种格式，但统一抽象到同一数据模型：

1. **兼容格式**：`<skill>/SKILL.md`
   - 继续支持现有 skill-creator 风格的目录；
   - frontmatter 或目录名提供最小元信息。

2. **目标格式**：

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

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | String | 唯一标识符 |
| `slug` | String | 文件系统中的路径标识 |
| `display_name` | String | 用户展示的显示名 |
| `description` | String | 简短描述（≤100字） |
| `instructions` | String | 注入 system prompt 的核心指令 |
| `tool_policy` | Enum | `inherit_all` / `allow_list` / `allow_list_with_deferred` |
| `sticky` | Boolean | true = 激活后不自动退出 |
| `aliases` | Vec<String> | 路由匹配别名 |
| `examples` | Vec<String> | 路由训练样本 |
| `source_path` | String | SKILL.md 或 skill.toml 的源路径 |
| `compat_mode` | Boolean | 兼容旧格式时标记 |

其中 `tool_policy` 不直接写成"工具白名单字符串数组"这么简单，而要支持三类模式：

- **`inherit_all`**：继承当前 agent 所有工具，适用于不限制能力的 skill
- **`allow_list`**：工具白名单列表，严格限制当前轮次可见工具
- **`allow_list_with_deferred`**：白名单 + 允许 ToolSearch 按需补充

这样可以覆盖：
- 普通 skill 只开放少量常驻工具；
- 高复杂度 skill 依赖 `ToolSearch` 再按需补充；
- 默认对话不激活任何 skill 时继续使用系统默认工具集。

#### 3.3 skill 生命周期

每个 session 维护一个显式 `ActiveSkillState`：

```
inactive ──► candidate ──► active ──► exiting
  ▲            │             │         │
  └────────────┴─────────────┴─────────┘
```

生命周期规则：
1. 用户消息进入后，先根据 active state 决定是否需要路由。
2. 若当前 skill 为 sticky，默认跳过重路由，继续沿用当前 skill。
3. 若切换 skill，旧 skill 的会话摘要写入 skill history segment，新 skill 进入 active。
4. 若模型或用户触发退出标记，active skill 退回 inactive。

这样既保留旧文档里的 sticky 思路，也避免当前"SkillTool 被当成普通工具调用后立即失效"的问题。

---

### 4. Tool 系统目标形态

#### 4.1 工具暴露分层

工具分为四组：

| 分组 | 工具 | 默认状态 |
|------|------|----------|
| 常驻基础工具 | `Bash`、`Read`、`Write`、`Edit` | always_loaded |
| 检索工具 | `WebSearch`、`WebFetch` | always_loaded |
| 编排工具 | `TaskCreate`、`TaskList`、`TaskUpdate`、`Agent` | deferred |
| 发现工具 | `ToolSearch`、`Skill` | deferred |

运行时不再默认把第 3、4 组全部暴露给模型，而是由 `CapabilityPolicy` 选择：

- 默认对话：基础工具 + 必要检索工具
- 复杂工作流 skill：加入任务编排工具
- 需要扩展能力时：暴露 `ToolSearch`
- 需要显式载入外部 skill 指令时：暴露 `Skill`

#### 4.2 deferred tool 的真实职责

当前 deferred tool 只是实现了"可延迟注册"，但还未进入完整工作流。目标上：

1. provider 初始只看见当前轮允许的 loaded tool + 一个 `ToolSearch`。
2. 模型判断当前工具不足时，通过 `ToolSearch` 请求 schema。
3. registry 将 deferred tool 提升为 loaded tool，并在后续迭代中可见。
4. event 层记录"哪个工具因何被解锁"，供 CLI / UI 展示。

这样才能真正实现 Claude Code 风格的"能力逐步暴露"，避免 system prompt 和 tool schema 一次性膨胀。

**关键实现点**：需要区分 **注册视图**（哪些工具在 registry 中 loaded + deferred）和 **观察视图**（当前轮次看到的 tool definitions）。当前 `ToolRegistry.tool_definitions()` 包含了所有 loaded + ToolSearch 只有一个入口，未来每轮应返回不同的 tool definition 集合。

#### 4.3 Task 工具的定位

Task 不只是"可选工具"，而应该成为复杂 skill 的标准编排层：

- 长流程 skill 默认启用 `TaskCreate/List/Update`；
- CLI / gateway / deskapp 都消费任务事件并展示进度；
- 后续可为计划型 skill 提供默认 task 模板；
- Task 需要支持 `parent_id` 层级关系（通过 `metadata` HashMap 实现）。

---

### 5. Prompt 与会话上下文

#### 5.1 system prompt 分层

目标 system prompt 由以下片段构成：

```
┌─────────────────────────────────────────┐
│ 1. Base prompt           (不变）         │
│ 2. Current agent prompt    (agent 规格)  │
│ 3. Active skill prompt     (session)     │
│ 4. Workflow/pending info  (工具/任务)     │
│ 5. Environment snapshot    (运行时状态)   │
│ 6. Tool usage guidance    (工具策略)      │
└─────────────────────────────────────────┘
```

`SystemPromptBuilder` 需要从"字符串累加器"升级为"具名 section builder"，以便：
- 做条件注入；
- 控制顺序；
- 在 CLI 中调试输出（`/prompt-sections`）；
- 测试时可断言具体 section 是否存在。

#### 5.2 历史管理

skill 切换后不保留整段原始消息，而是切成 segments：

```
全局摘要 ─────────── Per-Skill 摘要 ─── 当前 Active Segment
(name: string)      (id → Summary)      (Vec<Message>)
```

第一阶段先用**规则摘要**：
- 保留用户目标
- 保留关键决策
- 保留未完成事项
- 丢弃冗长工具日志

第二阶段（如有 LLM 路由能力）再扩展成 LLM 摘要。

---

### 6. 观测与评测

#### 6.1 运行时观测

| 事件 | 来源 | 位置 |
|------|------|------|
| `SkillLoaded` | SkillRegistry | `event.rs:60`（已有） |
| `SkillActivated` | SkillRouter | 新建 |
| `SkillSwitched` | SkillRouter | 新建 |
| `SkillExited` | SkillRouter | 新建 |
| `ToolUnlocked` | ToolRegistry | 新建 |
| `CapabilityPolicyChanged` | TurnContext | 新建 |
| `TaskStatusChanged` | TaskStore | `event.rs:52-56`（已有） |

#### 6.2 回归评测

| 指标 | 测试方法 |
|------|----------|
| Skill discovery 成功率 | 示例驱动测试（读取 `.nova/examples/*.json`） |
| Skill routing 稳定性 | 同一组消息重复路由 10 次，统计一致率 |
| Tool selection 正确率 | 模拟 `ToolSearch` 请求，验证 schema 返回 |
| Task 状态演进 | 验证完整 task 生命周期（创建 → 进行中 → 完成） |

---

## Plan 拆分与依赖关系

```
┌─────────────────┐     ┌─────────────────┐
│    Plan 1       │     │    Plan 1       │
│  能力模型与 Skill│     │  Skill 包协议    │
│  包协议统一      │────►统一              │
└─────────────────┘     └─────────────────┘
        │                       │
        ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│    Plan 2       │     │    Plan 3       │
│  Skill 路由、     │     │  Tool 暴露策略   │
│  激活态与 Prompt  │     │  Task 编排与     │
│  组装             │     │  ToolSearch 协同  │
└─────────────────┘     └─────────────────┘
        │                       │
        ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│    Plan 4       │     │    Plan 4       │
│  CLI / Gateway /  │───►│ DeskApp 集成    │
│  DeskApp 集成、   │     │  观测与评测     │
│  观测与评测       │     │                 │
└─────────────────┘     └─────────────────┘
```

### Plan 1：能力模型与 Skill 包协议统一

- **文件**：`docs/2026-04-24-tool-skill-capability-enhancement-plan-1.md`
- **目标**：把 skill / capability / prompt section 的核心数据模型先定义稳定，明确兼容策略和配置结构。
- **依赖**：无
- **风险**：需要决定 `SystemPromptBuilder` 升级是局部修改还是重构

### Plan 2：Skill 路由、激活态与 Prompt 组装

- **文件**：`docs/2026-04-24-tool-skill-capability-enhancement-plan-2.md`
- **目标**：让 skill 真正进入会话主流程，解决 active skill、sticky、历史切片、prompt 动态组装。
- **依赖**：Plan 1
- **风险**：`ActiveSkillState` 状态所有权归属（session layer vs runtime）

### Plan 3：Tool 暴露策略、Task 编排与 ToolSearch 协同

- **文件**：`docs/2026-04-24-tool-skill-capability-enhancement-plan-3.md`
- **目标**：把 deferred tool、task tool、agent tool 和 skill tool 串成统一 capability policy，减少 prompt/tool 噪音。
- **依赖**：Plan 1、Plan 2
- **风险**：`ToolRegistry` 需要支持 per-turn tool view 而不是全局视图

### Plan 4：CLI / Gateway / DeskApp 集成、观测与评测

- **文件**：`docs/2026-04-24-tool-skill-capability-enhancement-plan-4.md`
- **目标**：把前 3 个 plan 落到真实入口、事件协议、配置和测试里，形成可交付闭环。
- **依赖**：Plan 2、Plan 3

---

## 执行顺序建议

1. 先做 **Plan 1**，先把协议和状态模型定住。这是最安全的增量——只需新增类型，不影响现有行为。
2. 再做 **Plan 2**，让 skill 生命周期进入主链路。这是核心用户体验升级。
3. 然后做 **Plan 3**，收敛 tool 暴露与任务编排。这是性能优化（减少 token 消耗）。
4. 最后做 **Plan 4**，把 CLI / gateway / desktop 和评测打通。这是可观测性保障。

这个顺序的原因是：当前仓库最大的问题不是"缺少某个单独功能"，而是"原型存在但没有统一收口"。如果先做 UI 或单个 tool 增强，只会继续放大结构债务。

---

## 实现验证清单

实施前，下表列出需要验证的假设：

| 假设 | 验证方法 | 影响面 |
|------|----------|--------|
| `SystemPromptBuilder` 能用 section 模式工作 | 阅读 `crates/nova-core/src/prompt.rs` | Plan 1 & 2 |
| 当前所有 agent 共享全局 tool/skill registry | 确认 `nova-app/src/bootstrap.rs` 起始逻辑 | 全局 |
| CLI 前端能用 Tauri/stdio 接收新事件 | 确认 `nova-cli/src/main.rs` 事件循环 | Plan 4 |
| Gateway 事件桥接在 `nova-gateway-core` 中 | 定位 `nova-gateway-core/src/bridge.rs` 或等效文件 | Plan 4 |
| `.nova/examples/` 目录存在且可写 | 检查文件系统 | Plan 4 |
| LLM 路由可复用现有主模型配置 | 确认 `config.rs` 中有统一模型配置 | Plan 2 |

---

## Claude Code 真实会话数据验证

基于 `docs/something/v1_messages_20260423T132815Z` 原始通讯记录（2026-04-23，92 条消息，13.6 分钟会话）的分析：

### 7.1 Prompt Caching 开销分析

第一条请求的 token 数据：
- `input_tokens: 1`（本轮新 token）
- `cache_creation_input_tokens: 113`（首次缓存写入）
- `cache_read_input_tokens: 102733`（从缓存读取的 token 数！）

**关键发现**：
1. 系统提示词约 **102,733 tokens** 被缓存，每轮推理只需额外 ~1 token
2. **缓存热区**（多次读取的段落）应该优先保持紧凑
3. 设计中必须区分"**每次加载成本**"和"**每轮读取成本**"

### 7.2 实际 Prompt 分层结构

| 层次 | 标识（系统提示） | 预估占比 |
|------|-----------------|----------|
| 基础系统行为 | `You are Claude Code...` | ~0.5% |
| 任务执行指令 | `# Doing tasks` 块 | ~2% |
| **自动记忆** | `# auto memory`（**最长段**） | **~35%** |
| 环境变量 | `# Environment` | ~1% |
| Git 状态 | `gitStatus:` | ~0.5% |

**影响面**：auto memory 占 35%，设计文档需给出 memory 体积预算。

### 7.3 工具使用模式验证

| 工具 | 次数 | 使用场景 |
|------|------|----------|
| Bash | 17 | 目录探测、grep、文件存在性检查 |
| Read | 13 | 文件读取 |
| Write | 11 | 设计文档主写 |
| Edit | 8 | 局部修改 |
| TaskCreate | 8 | 大任务开始时批量创建 |
| TaskUpdate | 7 | 逐步标记完成 |
| **ToolSearch** | **1** | 仅加载 Task* 工具（上手晚） |
| Skill | 0 | 暴露但未使用 |
| Agent | 0 | 暴露但未使用 |

**验证的现有假设**：
- "上手晚了"正确——写到第 15 条消息才开始用 TaskCreate
- ToolSearch 只做了一件事（load deferred），说明"**选择加载所有 deferred**" vs "**按需加载**"之间的折中合理
- **Bash 主导探测阶段**，File tools 主导编写阶段——互补而非竞争

### 7.4 对设计的修正影响

| 修正项 | 优先级 | 涉及 Plan | 修改内容 |
|--------|--------|----------|----------|
| 增加 cache 预算约束 | P0 | Plan 1 & 2 | `CACHE_SECTION_MIN/MAX_TOKENS`、`SYSTEM_PROMPT_CACHE_TARGET` |
| SkillTool 三层模型 | P0 | Plan 3 | 会话级/工具级/用户级三层模型 |
| Task 动态加载时机 | P1 | Plan 3 | ToolSearch 优先，非静态暴露 |
| 文件工具 vs Bash 优先级 | P1 | Plan 1 | `file_tool_priority` 字段 |
| Session 压缩阶段定义 | P2 | Plan 2 | `window_size`、`compression_ratio`、`archive_behavior` |
| System-reminder 事件映射 | P2 | Plan 4 | 透传 vs 映射策略 |
| Memory 200 行限制 | P3 | Plan 1 | `MEMORY.md` 行数上限 |
| 工具控制权代理分配 | P3 | Plan 2 & 3 | 主代理 vs 子代理 |
