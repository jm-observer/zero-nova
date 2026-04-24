# 2026-04-24 tool-skill-capability-enhancement-plan-1

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 1：能力模型与 Skill 包协议统一 |
| 前置依赖 | 无 |
| 本次目标 | 定义稳定的 skill 元数据、capability policy、prompt section 和 session state 协议；兼容现有 `.nova/skills/*/SKILL.md`；明确配置与迁移路径，为后续运行时接入提供统一数据模型。 |
| 涉及文件 | `crates/nova-core/src/skill.rs`、`crates/nova-core/src/config.rs`、`crates/nova-core/src/prompt.rs`、`crates/nova-core/src/agent.rs`、`.nova/README.md`、`.nova/examples/agents.toml`、`docs/2026-04-24-tool-skill-capability-enhancement.md` |
| 代码验证状态 | 已确认 (2026-04-24) |

---

## 详细设计

### 1. Skill 数据模型重构

#### 1.1 当前状态（已验证 `crates/nova-core/src/skill.rs`）

```rust
// 现有模型 — 仅包含 4 个字段
pub struct Skill {
    pub name: String,        // 支持 name: 和 description: 前缀行解析
    pub description: String,
    pub body: String,        // SKILL.md 正文
    pub path: PathBuf,       // 父目录路径
}
```

当前 `SkillRegistry` 使用 `---` 分割 content（line 58），匹配 `name:` / `description:` 行（line 82-86）。若无 frontmatter，使用目录名作为 fallback。

**`generate_system_prompt()` 方法**（line 107-122）将所有 skill 拼入一个字符串。

#### 1.2 目标模型

```rust
pub struct SkillPackage {
    pub id: String,          // 唯一标识符（推荐使用 slug）
    pub slug: String,        // 文件系统中的路径标识
    pub display_name: String,
    pub description: String, // ≤100字
    pub instructions: String, // 注入 system prompt 的核心指令
    pub tool_policy: ToolPolicy,
    pub sticky: bool,
    pub aliases: Vec<String>,
    pub examples: Vec<String>, // 路由训练样本
    pub source_path: PathBuf, // 来源文件路径
    pub compat_mode: bool,    // 兼容旧格式时标记
}
```

#### 1.3 三种 ToolPolicy 模式

```rust
pub enum ToolPolicy {
    InheritAll,              // 继承当前 agent 所有工具
    AllowList(Vec<String>),  // 严格工具白名单
    AllowListWithDeferred(Vec<String>), // 白名单 + ToolSearch 补充
}
```

三者覆盖：
- 普通 skill 只开放少量常驻工具；
- 高复杂度 skill 依赖 `ToolSearch` 再按需补充；
- 默认对话不激活任何 skill 时继续使用系统默认工具集。

#### 1.4 兼容策略

1. **兼容模式**：若目录下只有 `SKILL.md`，按兼容模式加载：
   - `slug` 默认使用相对路径（`skill_dir.file_name()`）
   - `display_name` 默认取目录名
   - `instructions` 取 markdown 正文（等价于现有 `body`）
   - `tool_policy` 默认 `ToolPolicy::InheritAll`
   - `sticky` 默认 `false`

2. **目标模式**：若存在 `skill.toml`，则以结构化字段为准，`SKILL.md` 仅作说明文档或 fallback。

#### 1.5 `skill.toml` 设计建议

```toml
# .nova/skills/<slug>/skill.toml
id = "skill-creator"
slug = "skill-creator"
display_name = "Skill Creator"
description = "Create new skills for the agent"
instructions = """
You are a skill creator. When asked to create a new skill, follow these steps...
"""
sticky = true
aliases = ["skill-creator", "skill-creation"]

# 工具策略
tool_policy.allow_list = ["Bash", "Read", "Write", "Edit", "Skill"]
tool_policy.with_deferred = ["TaskCreate", "TaskList", "ToolSearch"]

examples = [
    "create a skill for code review",
    "create a skill for testing",
]
```

---

### 2. SkillRegistry 的职责收敛

#### 当前状态（已验证）

- `SkillRegistry` 只负责读一层目录并拼 prompt
- 当前使用 `std::fs::read_dir`（非异步），见 `skill.rs:29-36`
- 不包含任何运行时逻辑（路由、状态更新等）

#### 改造后职责拆为：

1. **递归扫描 skill 根目录** — 将当前 `load_from_dir` 从单层改为递归
2. **解析技能包并生成 `SkillPackage` 列表** — 支持 `skill.toml` 和 `SKILL.md` 两种格式
3. **提供按 `id` / `slug` / `alias` 查询** — 新增 `find_by_slug()`、`find_by_name()` 等方法
4. **提供供路由器使用的候选清单** — 新增 `all_candidates()` 返回所有可用 skill
5. **不再负责直接生成整包 system prompt** — 改为 `get_skill_prompt(slug)` 独立查询

**这样做的原因：**
- prompt 组装是 turn 级逻辑，不该耦合在 registry；
- registry 应该是"静态定义层"，而不是"运行时渲染层"。

---

### 3. CapabilityPolicy 模型

#### 3.1 模型定义

新增统一策略对象，用于描述当前轮次可见能力：

```rust
pub struct CapabilityPolicy {
    pub always_enabled_tools: Vec<String>, // e.g. ["Bash", "Read", "Write", "Edit"]
    pub deferred_tools: Vec<String>,        // e.g. ["TaskCreate", "Skill"]
    pub tool_search_enabled: bool,          // 允许 ToolSearch 按需加载
    pub skill_tool_enabled: bool,           // 允许技能补充加载
    pub task_tools_enabled: bool,           // 允许 Task 工具
    pub agent_tools_enabled: bool,          // 允许 Agent 子代理
    pub source: PolicySource,               // 策略来源追踪

    // Cache 预算约束（基于 v1_messages 会话分析，102,733 tokens 缓存）
    pub cache_section_min_tokens: usize,    // 触发缓存创建的最小段（100）
    pub cache_section_max_tokens: usize,    // 单个 cache section 上限（4000）
    pub system_prompt_cache_target: usize,  // 目标缓存大小（98000）
    pub file_tool_priority: FileToolPriority, // 文件 vs Bash 优先级
}
```

**`FileToolPriority` 枚举** — 文件工具优先性（基于工具使用模式分析）：

```rust
pub enum FileToolPriority {
    PreferFileTools,  // 优先 Read/Write/Edit，失败时 fallback 到 Bash
    PreferBash,       // 优先 Bash，适用于大量 shell 操作场景
    Adaptive,         // 根据操作类型自适应（读 → 文件工具，探测 → Bash）
}
```

**`PolicySource` 枚举** — 记录策略来源，便于调试和回溯：

```rust
pub enum PolicySource {
    Default,           // 运行入口默认策略
    AgentSpec,         // 当前 agent 规格
    ActiveSkill,       // active skill 的 tool_policy
    UserOverride,      // 用户显式模式切换
}
```

#### 3.2 策略合成优先级

策略来源按优先级合成（低 → 高）：

```
运行入口默认 ──► 当前 agent 规格 ──► active skill ──► 用户覆盖
```

- **运行入口默认** (`Default`)：CLI 启动时根据运行模式确定。CLI 默认 `tool_search_enabled=true, task_tools_enabled=false`；Gateway 默认 `tool_search_enabled=true, task_tools_enabled=true`。
- **当前 agent 规格** (`AgentSpec`)：从 `config.rs` 读取的 `AgentSpec` 中的 `tool_whitelist` 转换为策略。
- **active skill** (`ActiveSkill`)：当 `active_skill` 存在且 `sticky=false` 时，使用其 `tool_policy`。
- **用户覆盖** (`UserOverride`)：在会话期间手动修改的临时策略。

**影响：**
- this model becomes the public foundation for Plans 2 & 3, avoiding scattered `tool_whitelist: Option<Vec<String>>` checks everywhere.

---

### 4. PromptSectionBuilder 抽象

#### 当前状态（已验证 `crates/nova-core/src/prompt.rs`）

`SystemPromptBuilder` 使用 `Vec<String>` + `build()` 模式：
- 有 `role()`、`guideline()`、`environment()`、`custom_instruction()`、`extra_section()` 方法
- 输出是一个拼接字符串

#### 改造方案 — section-based builder

```rust
pub struct PromptSectionBuilder {
    sections: Vec<NamedSection>,
}

pub struct NamedSection {
    pub name: SectionName,  // 具名 section
    pub content: String,
    pub required: bool,     // 是否需要的内容
    pub priority: Priority, // 注入优先级
}

pub enum SectionName {
    Base,
    Agent,
    Skill,
    Environment,
    Workflow,
    ToolGuidance,
    History,
}

pub enum SectionPriority {
    High,   // 总是插入
    Medium, // 条件插入（如 active skill）
    Low,    // 仅调试或覆盖模式插入
}
```

#### 关键增强点

1. **`NamedSection` 支持独立构造** — 每个 section 单独准备内容，最后统一 `build()`
2. **Section 级调试接口** — 新增 `debug_sections()` 返回 section 列表，便于 CLI 输出 `/prompt-sections`
3. **条件注入** — 基于 `required` 和 `priority` 决定 section 是否最终出现在 prompt 中
4. **顺序控制** — `sections` 中的 `priority` 决定最终输出顺序

---

### 5. 配置结构扩展

**当前状态（已验证 `crates/nova-core/src/config.rs`）**

现有配置已包含必要基础：
- `ToolConfig.skills_dir: Option<String>` — 已存在 (line 86)
- `GatewayConfig.agents: Vec<AgentSpec>` — Agent 规格列表
- `AgentSpec.tool_whitelist: Option<Vec<String>>` — 工具白名单
- `GatewayConfig.max_iterations: usize` — 会话轮次限制
- `GatewayConfig.subagent_timeout_secs: u64` — 子代理超时

**新增配置字段建议：**

```rust
pub struct GatewayConfig {
    // ...existing fields...
    pub skill_routing_enabled: bool,       // 是否启用自动 skill 路由
    pub skill_history_strategy: String,    // "global" | "per_skill" | "segments"
}
```

```rust
pub struct ToolConfig {
    // ...existing fields...
    pub default_policy: Option<String>,    // "minimal" | "full" | "workflow"
}
```

**约束：**
- 必须给出默认值，保证旧配置可直接运行（使用 `#[serde(default = "...")]`）
- 优先使用字符串枚举和简单结构，避免一次性引入复杂嵌套
- `skill_history_strategy` 的三个阶段对应 Plan 1/2/3 的演进：
  - `global`：兼容现有全量历史
  - `per_skill`：Plan 2 阶段 - 按 skill 分割
  - `segments`：Plan 3 阶段 - 规则摘要 + segment 裁剪

---

### 6. 实现步骤与优先级

| 步骤 | 内容 | 文件 | 风险 |
|------|------|------|------|
| 1 | 定义 `SkillPackage`、`CapabilityPolicy`、`ToolPolicy` 类型 | `nova-core/src/skill.rs` | 低 - 仅新增类型 |
| 2 | 重构 `SkillRegistry` - 递归扫描 + 兼容模式 | `nova-core/src/skill.rs` | 低 - 向后兼容 |
| 3 | 新增 `skill.toml` 解析 | `nova-core/src/skill.rs` | 低 - 可选加载 |
| 4 | 升级 `SystemPromptBuilder` 为 section-based | `nova-core/src/prompt.rs` | 中 - 影响 prompt 生成 |
| 5 | 扩展 `config.rs` 配置结构 | `nova-core/src/config.rs` | 低 - 仅新增字段 |
| 6 | 更新 `.nova/examples/` 目录中的示例配置 | `.nova/examples/` | 低 |

---

## 测试案例

1. **正常路径**：从 `.nova/skills/skill-creator/SKILL.md` 成功加载 skill，生成稳定的 `slug`、`display_name`、`instructions`。
2. **正常路径**：递归扫描嵌套目录，确保不仅限于一级子目录。
3. **边界条件**：`SKILL.md` 无 frontmatter 时，仍能按目录名生成兼容 skill（`compat_mode=true`）。
4. **边界条件**：同名 `slug` 或 alias 冲突时，返回带上下文的错误。
5. **异常场景**：`skill.toml` 存在但格式非法，加载失败且错误信息指向具体文件。
6. **正常路径**：Prompt builder 能按 section 顺序输出，不因某 section 缺失而产生重复空白。
7. **异常场景**：旧配置文件缺少新字段时，`AppConfig` 仍能成功加载并落到默认策略（验证 `serde(default)` 正确使用）。
8. **新测试**：验证 `CapabilityPolicy` 合成逻辑 - 当 active skill 存在、agent spec 也有 whitelist 时，优先使用 active skill。
9. **新测试**：验证三种 `ToolPolicy` 模式生成的 `CapabilityPolicy` 不同结果。
10. **新测试**：验证 `SkillRegistry::find_by_slug()` 和 `find_by_alias()` 正确返回对应 skill。
