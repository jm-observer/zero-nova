# Skill 系统设计文档

> 版本: v2.0 | 日期: 2026-04-17

## 1. 背景与动机

当前 `AgentRuntime` 使用单一的 `system_prompt` 和完整的 `ToolRegistry` 处理所有对话。这意味着：

| 问题 | 影响 |
|------|------|
| 系统提示词膨胀 | 所有场景的指令堆在一个 prompt 中，token 浪费严重，LLM 注意力被稀释 |
| 工具集无法按需裁剪 | 每次请求都暴露全部工具定义，增加不必要的 token 开销和误调用风险 |
| 缺乏行为模式切换 | 不同任务（代码提交、文档审查、技术咨询）需要不同的行为规则，目前无法动态切换 |
| 历史消息无差异化管理 | skill 切换后，旧 skill 的对话历史成为噪音，无法压缩或裁剪 |

### 1.1 目标

引入 **Skill 系统**，实现：

1. **按需加载提示词** — 根据用户输入动态注入对应 skill 的 prompt，基础 prompt 保持精简
2. **工具集过滤** — 每个 skill 声明可用工具白名单，只暴露必要的工具
3. **用户可扩展** — skill 通过外部文件定义（Markdown + TOML），用户无需修改代码即可新增/修改 skill
4. **动态层级路由** — 支持任意深度的目录组织，通过单次 LLM 分类完成路由
5. **历史消息智能管理** — skill 切换时对旧历史做摘要压缩，保留跨 skill 上下文引用

## 2. 核心概念

### 2.1 节点类型

Skill 系统的目录结构是一棵**树**，树中的节点分为两种类型：

| 节点类型 | 判定依据 | 作用 |
|---------|---------|------|
| **Skill（叶子节点）** | 目录中包含 `skill.toml` + `prompt.md` | 可被激活，包含行为提示词 |
| **Group（分组节点）** | 目录中包含子目录，但不含 `skill.toml` | 仅用于组织管理，不可被激活 |

**关键规则：只有叶子节点（含 `skill.toml` 的目录）会被注册为可激活的 skill。** Group 节点仅提供目录层级上的分类组织，分类器不感知它们。

### 2.2 目录结构

支持任意深度的嵌套：

```
skills/
├── coding/                          ← Group（无 skill.toml，仅分组）
│   ├── commit/                      ← Skill（叶子）  slug = "coding/commit"
│   │   ├── skill.toml
│   │   └── prompt.md
│   ├── review/                      ← Skill（叶子）  slug = "coding/review"
│   │   ├── skill.toml
│   │   └── prompt.md
│   └── debug/                       ← Group
│       ├── frontend/                ← Skill（叶子）  slug = "coding/debug/frontend"
│       │   ├── skill.toml
│       │   └── prompt.md
│       └── backend/                 ← Skill（叶子）  slug = "coding/debug/backend"
│           ├── skill.toml
│           └── prompt.md
├── writing/                         ← Group
│   ├── blog/                        ← Skill（叶子）  slug = "writing/blog"
│   │   ├── skill.toml
│   │   └── prompt.md
│   └── docs/                        ← Skill（叶子）  slug = "writing/docs"
│       ├── skill.toml
│       └── prompt.md
└── tech-consult/                    ← Skill（叶子，根级别） slug = "tech-consult"
    ├── skill.toml
    └── prompt.md
```

**slug 规则：** slug 是 skill 目录相对于 `skills/` 根目录的路径，使用 `/` 分隔。例如 `skills/coding/debug/frontend/` 的 slug 为 `coding/debug/frontend`。

### 2.3 扁平化分类策略

> **设计决策：** 不管目录结构有多深，分类器**只做一次调用**。所有叶子节点被展平为一个 slug + description 列表传给分类器。
>
> 目录层级仅用于**用户侧的组织管理**，不影响分类逻辑。

```
分类器看到的候选列表（自动从树中提取所有叶子）：

- "coding/commit"           — 帮助用户进行代码提交
- "coding/review"           — 审查代码质量
- "coding/debug/frontend"   — 调试前端问题
- "coding/debug/backend"    — 调试后端问题
- "writing/blog"            — 撰写博客文章
- "writing/docs"            — 编写技术文档
- "tech-consult"            — 技术咨询

用户: "帮我调试前端的 bug"
→ 一次 LLM 调用 → {"skill": "coding/debug/frontend"}
→ 加载 skills/coding/debug/frontend/prompt.md
```

**为什么不用递归分类（每层一次 LLM 调用）：**

| 方案 | 调用次数 | 延迟 | 适用场景 |
|------|---------|------|---------|
| 扁平化单次分类 | 1 次 | 200-500ms | skill 总数 < 50（绝大多数场景） |
| 递归分类 | N 次（= 层级深度） | N × 200-500ms | skill 总数 > 50，单次候选集过大 |

当前设计选择**扁平化单次分类**。如果未来 skill 数量增长到需要递归分类的程度，可在 `SkillRouter` 内部切换策略，不影响外部接口。

### 2.4 skill.toml 格式

```toml
[skill]
name = "commit"
description = "帮助用户进行代码提交，包括查看变更、生成 commit message、执行 git 操作等"
version = "1.0"

[tools]
# 工具白名单，空列表 = 使用全部工具
allowed = ["bash"]

[config]
# skill 级别的自定义配置（可选）
max_iterations = 5
```

> **注意：** 没有 `[trigger]` 配置。Skill 路由完全通过 LLM 意图分类实现，分类器根据每个 skill 的 `description` 来判断用户消息应匹配哪个 skill。因此 `description` 字段应当清晰准确地描述该 skill 的适用场景。

### 2.5 系统层级

```
┌─────────────────────────────────────────────────┐
│              API 请求构成                         │
├─────────────────────────────────────────────────┤
│                                                 │
│  system:                                        │
│    ┌─────────────────────────┐                  │
│    │ base prompt              │  ← 精简基础人格   │
│    │ (prompts/base.md)        │                  │
│    ├─────────────────────────┤                  │
│    │ skill prompt             │  ← 按需注入      │
│    │ (skills/.../prompt.md)   │    每轮动态替换   │
│    ├─────────────────────────┤                  │
│    │ environment info         │  ← 运行时环境信息 │
│    └─────────────────────────┘                  │
│                                                 │
│  messages:                                      │
│    ┌─────────────────────────┐                  │
│    │ 旧 skill 对话摘要        │  ← 压缩后的历史   │
│    ├─────────────────────────┤                  │
│    │ 当前 skill 完整对话      │  ← 完整保留      │
│    └─────────────────────────┘                  │
│                                                 │
│  tools:                                         │
│    当前 skill 的工具子集                          │
│    (或无 skill 时使用全部工具)                     │
│                                                 │
└─────────────────────────────────────────────────┘
```

### 2.6 Skill 生命周期

```
用户消息到达
     │
     ▼
┌──────────────────────────────────┐
│  Skill 路由                       │
│  (小模型 LLM 意图分类)             │
│                                    │
│  输入：用户消息 +                   │
│       所有叶子 skill 的 slug/desc  │
│       (扁平化列表，无论层级多深)     │
│                                    │
│  输出：skill slug 或 null          │
│       (如 "coding/debug/frontend") │
└──────────────┬─────────────────────┘
               │
         ┌─────┴─────┐
         │ 匹配成功?  │
         └─────┬─────┘
      是       │       否
       ▼       │       ▼
┌────────────┐ │  ┌──────────────┐
│ 按 slug     │ │  │ 使用 base     │
│ 查找 skill  │ │  │ prompt only  │
│ 加载 prompt │ │  └──────┬───────┘
└──────┬─────┘ │         │
       │       │         │
       ▼       ▼         ▼
┌──────────────────────────────────────┐
│ 检查 skill 是否切换                    │
│  └─ 是 → 对旧历史做摘要压缩           │
│  └─ 否 → 保留完整历史                 │
└──────────────┬───────────────────────┘
               │
               ▼
┌──────────────────────────────────────┐
│ 过滤工具集                            │
│  └─ 有白名单 → 只暴露白名单内工具      │
│  └─ 无白名单 → 暴露全部工具            │
└──────────────┬───────────────────────┘
               │
               ▼
        调用主模型 LLM API
        进入 Agent Loop
```

## 3. 架构设计

### 3.1 新增模块

```
src/
├── skill/
│   ├── mod.rs          # 模块入口，公开 API
│   ├── definition.rs   # SkillDefinition 结构体，TOML 解析
│   ├── registry.rs     # SkillRegistry，递归加载与管理所有 skill
│   ├── router.rs       # SkillRouter，LLM 意图分类路由
│   └── history.rs      # 历史消息管理，摘要压缩逻辑
```

### 3.2 核心数据结构

```rust
/// Skill 的工具约束
pub struct SkillToolConstraint {
    pub allowed: Vec<String>,     // 工具白名单，空 = 全部
}

/// Skill 定义（叶子节点，可被激活）
pub struct SkillDefinition {
    pub slug: String,             // 相对路径标识，如 "coding/debug/frontend"
    pub name: String,
    pub description: String,      // 分类器依据此字段判断匹配
    pub prompt: String,           // 从 prompt.md 加载的内容
    pub tools: SkillToolConstraint,
    pub config: SkillConfigOverride,
}

/// Skill 注册表（只存储叶子节点，扁平化）
pub struct SkillRegistry {
    skills: Vec<SkillDefinition>,
}

/// Skill 路由器（基于 LLM 意图分类，扁平化单次调用）
pub struct SkillRouter {
    classifier_prompt: String,    // 启动时根据所有叶子 skill 构建
    classifier_config: ModelConfig,
}

/// 路由结果
pub struct RouteResult<'a> {
    pub skill: Option<&'a SkillDefinition>,
}

/// 完整的轮次上下文
pub struct TurnContext {
    pub skill: Option<SkillDefinition>,
    pub system_prompt: String,
    pub tool_definitions: Vec<ToolDefinition>,
    pub history: Vec<Message>,
    pub active_skill: Option<String>,
}
```

### 3.3 目录扫描策略

`SkillRegistry` 在启动时**递归扫描** skill 目录：

```
递归扫描 skills/
  │
  ├─ 发现 skills/coding/commit/skill.toml
  │   → 注册为 SkillDefinition { slug: "coding/commit", ... }
  │
  ├─ 发现 skills/coding/debug/frontend/skill.toml
  │   → 注册为 SkillDefinition { slug: "coding/debug/frontend", ... }
  │
  ├─ 发现 skills/tech-consult/skill.toml
  │   → 注册为 SkillDefinition { slug: "tech-consult", ... }
  │
  └─ skills/coding/ 无 skill.toml → 跳过（Group 节点）
     skills/coding/debug/ 无 skill.toml → 跳过（Group 节点）
```

**结果：** registry 中只存储叶子节点，是一个**扁平列表**。层级信息仅体现在 slug 的路径中。

### 3.4 与现有模块的交互

```
                      ┌──────────────────┐
                      │  SkillRegistry    │
                      │  (递归扫描目录)    │
                      │  (只存储叶子节点)  │
                      └──────┬───────────┘
                             │ 扁平化的 slug + description 列表
                             │
┌──────────┐          ┌──────▼──────────┐       ┌────────────────┐
│ 用户消息  │ ────────▶│ SkillRouter      │ ─────▶│ SystemPrompt   │
│          │          │ (小模型 LLM 分类) │       │ Builder        │
└──────────┘          │ (单次调用)       │       │ (组装 prompt)  │
                      └──────┬──────────┘       └───────┬────────┘
                             │                          │
                      ┌──────▼──────┐           ┌───────▼────────┐
                      │ HistoryMgr   │           │ ToolRegistry   │
                      │ (历史管理)    │           │ (工具过滤)     │
                      └──────┬──────┘           └───────┬────────┘
                             │                          │
                             └────────┬─────────────────┘
                                      │
                               ┌──────▼──────┐
                               │ AgentRuntime │
                               │ .run_turn()  │  ← 主模型
                               └─────────────┘
```

**关键改动点：**

| 现有模块 | 改动 | 说明 |
|----------|------|------|
| `AgentRuntime` | `system_prompt` 从固定值改为每轮动态传入 | `run_turn()` 接受 `TurnContext` 参数 |
| `SystemPromptBuilder` | 新增 `from_base()` 和 `.with_skill()` | 注入 skill prompt 片段 |
| `ToolRegistry` | 新增 `tool_definitions_filtered()` | 返回工具定义的子集 |
| `Session` | 新增 `active_skill` 字段 | 跟踪当前会话的活跃 skill |
| `handle_chat()` | 在调用 `run_turn()` 前插入 skill 路由逻辑 | 路由 → 组装 → 过滤 → 执行 |
| `config.rs` | 新增 `SkillConfig` + `ClassifierConfig` | skill 目录、分类器模型配置 |

### 3.5 Prompt 组装策略

每次 API 请求时，system prompt 动态组装：

```rust
fn build_system_prompt(
    base: &str,
    skill: Option<&SkillDefinition>,
    env: &EnvironmentInfo,
) -> String {
    let mut builder = SystemPromptBuilder::from_base(base);

    if let Some(skill) = skill {
        builder = builder.with_skill(skill);
    }

    builder = builder.with_environment(env);
    builder.build()
}
```

**system prompt 永远不进入 messages 历史**，它是每次请求独立传入 Anthropic API 的 `system` 字段。

### 3.6 历史消息管理策略

当 skill 切换时，需要对旧历史做处理：

| 场景 | 策略 |
|------|------|
| 同一 skill 内连续对话 | 完整保留历史 |
| skill A → skill B 切换 | 对 skill A 的历史做摘要，skill B 的对话完整保留 |
| skill → 无 skill | 对 skill 历史做摘要 |
| 无 skill → skill | 对旧历史做摘要 |

摘要压缩有两种实现路径：

1. **LLM 摘要**（质量高，有延迟和成本）— 调 LLM 生成一段摘要
2. **规则摘要**（简单快速）— 只保留用户消息的文本部分，丢弃工具调用细节

建议默认使用**规则摘要**，LLM 摘要作为可选高级策略。

### 3.7 配置扩展

```toml
# config.toml 新增
[skill]
# skill 定义文件目录（支持任意深度嵌套）
directory = "./skills"
# 是否启用 skill 系统
enabled = true
# 历史摘要策略: "rule" | "llm"
history_strategy = "rule"

[skill.classifier]
# 分类器模型（小模型，快速低成本）
model = "claude-3-5-haiku-latest"
max_tokens = 128
# 可选：分类器使用独立的 API 配置，不设则复用主 LLM 配置
# base_url = "https://api.anthropic.com"
# api_key = "..."
```

## 4. 详细设计拆分

本设计拆分为三个实施阶段，每个阶段对应一份详细设计文档：

| 阶段 | 文档 | 内容 |
|------|------|------|
| Plan 1 | `plans/skill-plan-1-definition-and-loading.md` | Skill 定义格式、递归目录扫描、SkillRegistry 实现、配置扩展 |
| Plan 2 | `plans/skill-plan-2-routing-and-prompt.md` | LLM 分类器设计、扁平化候选列表、SystemPromptBuilder 改造、工具过滤、与 AgentRuntime 集成 |
| Plan 3 | `plans/skill-plan-3-history-management.md` | 历史消息分段、摘要压缩、Session 改造 |

各阶段可独立实施和测试，Plan 1 是 Plan 2 的前置依赖，Plan 3 可与 Plan 2 并行开发。

## 5. 对外接口变化

### 5.1 Gateway 协议扩展（可选）

```json
// 新增：查询可用 skills（返回扁平化列表，含层级路径）
{ "type": "skills.list" }
// 响应
{ "type": "skills.list.response", "skills": [
  { "slug": "coding/commit", "name": "commit", "description": "帮助用户进行代码提交" },
  { "slug": "coding/debug/frontend", "name": "frontend", "description": "调试前端问题" },
  { "slug": "tech-consult", "name": "tech-consult", "description": "技术咨询" }
]}
```

### 5.2 Chat 消息扩展

```json
// chat 响应中可携带当前 skill 信息（slug 含路径）
{ "type": "chat.start", "session_id": "...", "active_skill": "coding/debug/frontend" }
```

## 6. 不做的事情

- **不做递归分类** — 当前只做单次扁平化分类，不按层级逐级分类（未来 skill 数量超过 50 时可考虑）
- **不做 Group 级别的配置继承** — Group 不参与分类，不支持在 Group 上定义可被子 skill 继承的配置
- **不做 skill 间依赖** — 每个 skill 独立，不支持 skill 组合或链式调用
- **不做热重载** — 第一版 skill 在启动时加载，运行时修改需重启
- **不做 skill 沙箱** — skill prompt 完全信任，不做隔离或权限限制
- **不做多 skill 并行激活** — 每轮只激活一个 skill 或不激活

## 7. 待定设计：Sticky 机制与多轮确认流程

> 状态: 讨论中 | 日期: 2026-04-17

### 7.1 问题背景

当前设计假设 skill 是**预定义的、特定领域的**（如 commit、review）。但实际使用中存在一类**通用多轮流程型**场景：

用户提出某个领域的技术需求（TTS、图片生成、编程框架等），agent 需要完成：
1. 搜索主流方案，整理对比
2. 用户选择方案
3. 下载模型 / 拉取 Docker 镜像 / 安装依赖
4. 部署运行
5. 根据用户要求测试

这类场景的关键特点：
- **skill 不是特定领域的**，而是"方案搜索与部署"这种通用流程描述
- **不可穷举**，不可能为每个领域（TTS、图片生成……）都预建一个 skill
- **需要多轮用户确认**，每个关键步骤前停下来等用户决策
- **跨越多轮对话**，可能 5-10 轮以上

### 7.2 设计思路：LLM 自然对话 + 用户多轮确认

不引入系统级 workflow 引擎。多阶段流转完全由 LLM 在对话中自然完成，通过 prompt 指引在关键节点停下来请求用户确认。

示例对话流：

```
用户: "我想要一个 TTS 方案"
  → 分类器匹配到 solution-deploy skill
  → LLM: "我找到了这几个方案：1. Coqui TTS  2. Fish Speech  3. GPT-SoVITS，你选哪个？"
用户: "2"
  → LLM: "Fish Speech 支持 Docker 部署，需要我帮你拉镜像并启动吗？"
用户: "好的"
  → LLM: 执行 docker pull / docker run
  → LLM: "已经启动了，要测试一下吗？"
用户: "用这段文字测试..."
  → LLM: 调用 API 测试
```

skill 的 prompt.md 只需描述通用流程框架，而非特定领域知识：

```markdown
# 方案搜索与部署

你是一个技术方案顾问。当用户提出某个技术需求时，按以下流程执行：

1. **搜索阶段** — 使用 web_search 搜索当前主流方案，整理为对比表格
2. **选型阶段** — 向用户呈现方案对比，等待用户选择
3. **部署阶段** — 根据选定方案，下载模型/拉取镜像/安装依赖
4. **测试阶段** — 启动服务，根据用户要求进行功能验证

**重要：每个阶段结束后，必须等待用户确认才能进入下一阶段。**
```

### 7.3 Sticky 机制

此类多轮流程的核心风险是：对话进行到一半，分类器误判切换了 skill，导致上下文丢失。

解决方案：给 skill 新增 `sticky` 属性，激活后锁定，不被分类器切走。

#### skill.toml 扩展

```toml
[skill]
name = "solution-deploy"
description = "搜索技术方案并部署运行"
sticky = true    # 默认 false
```

大部分简单 skill（commit、review 等一问一答型）不需要 sticky。只有多轮确认的长流程 skill 才设为 true。

#### 路由器逻辑变化

原设计每轮无条件分类，改为：

```
用户消息到达
     │
     ▼
┌─────────────────────────┐
│ 当前是否有 active_skill？ │
└──────────┬──────────────┘
           │
     ┌─────┴─────┐
     │   是       │   否
     ▼           ▼
┌─────────┐  ┌──────────────┐
│ sticky?  │  │ 正常走分类器  │
└────┬────┘  └──────┬───────┘
  是  │  否          │
  │   ▼             │
  │  正常走分类器    │
  ▼                 │
 跳过分类，          │
 继续当前 skill      │
        │            │
        └──────┬─────┘
               ▼
         组装 TurnContext
```

```rust
fn route(&self, message: &str, active_skill: Option<&SkillDefinition>) -> RouteResult {
    // sticky skill 锁定期间跳过分类
    if let Some(skill) = active_skill {
        if skill.sticky {
            return RouteResult { skill: Some(skill) };
        }
    }
    // 否则正常走 LLM 分类
    self.classify(message)
}
```

#### 退出机制

sticky skill 需要退出方式，否则永远锁定。两个途径并存：

**途径 A：LLM 主动退出**

当 LLM 判断任务完成或用户明确放弃时，输出特殊标记：

```markdown
<!-- 在 sticky skill 的 prompt 末尾追加 -->
当你判断任务已完成或用户明确表示不再继续时，在回复末尾附加：
<skill_exit/>
```

AgentRuntime 检测到此标记后清除 `active_skill`。

**途径 B：用户显式退出**

用户输入 `/exit` 或 `/reset` 强制退出当前 skill。作为兜底，防止 LLM 判断失误导致卡死。

### 7.4 对原设计各模块的影响

| 模块 | 改动 |
|------|------|
| `SkillDefinition` | 新增 `sticky: bool` 字段 |
| `skill.toml` 解析 | 解析 `sticky`，默认 false |
| `SkillRouter.route()` | 增加 sticky 判断逻辑，已锁定则跳过分类 |
| `Session` | `active_skill` 已有，无需改动 |
| `AgentRuntime` | 解析 LLM 输出时检测 `<skill_exit/>`，清除 active_skill |
| 历史管理 | sticky 期间不触发 skill 切换，不压缩，无改动 |
| 配置 | 无改动 |

### 7.5 待讨论：Sticky 期间的无关消息处理

sticky 锁定期间，如果用户问了与当前 skill 完全无关的问题，两种处理策略：

| 策略 | 描述 | 优劣 |
|------|------|------|
| **严格锁定** | 仍用当前 skill 的 prompt 和工具集处理 | 简单可靠，但可能回答不好无关问题 |
| **智能判断** | 仍让分类器跑一次，高置信度判断为其他 skill 时才切换 | 更灵活，但引入复杂度，可能误切 |

暂定先实现**严格锁定**，后续按需加智能判断。
