# 提示词架构增强设计

- 日期：2026-04-25
- 状态：草案（v2 — 基于代码审查的修订版）
- 范围：nova-core prompt 系统、bootstrap 流程、skill/workflow 注入

---

## 一、Claude Code 提示词机制补充分析

原分析文档（`docs/todo/2026-04-24-claude-code-usage-analysis.md`）已涵盖了 Claude Code 的分层注入架构、tool 使用方式、skill 机制等。以下是补充的关键机制细节。

### 1.1 分层注入的完整结构（修正版）

Claude Code 的提示词实际分为 **7 层**，比原文档分析的 6 层更细：

| 层级 | 内容 | 注入位置 | 变化频率 |
|------|------|----------|----------|
| L0 — 平台身份 | 角色定义、安全边界、行为准则、输出风格、git 操作规范、PR 创建流程 | system prompt | 几乎不变 |
| L1 — 工具能力 | 已加载工具的完整 JSON Schema + 延迟工具名录（仅名称） | tools 参数 + system prompt | 工具变更时 |
| L2 — Skills 目录 | 可用 skill 列表及触发条件描述 | system-reminder 标签 | skill 配置变更时 |
| L3 — 项目上下文 | CLAUDE.md 全量注入（构建命令、架构说明、代码规范） | system-reminder 标签 | 用户编辑时 |
| L4 — 记忆规则 | auto memory 的类型定义（user/feedback/project/reference）、读写规范、MEMORY.md 维护规则 | system prompt | 几乎不变 |
| L5 — 环境快照 | CWD、git branch、git status、最近提交、平台、shell、模型版本 | system prompt（仅首次） | 每会话一次 |
| L6 — 会话内侧信道 | 通过 `<system-reminder>` 标签嵌入 tool result 或 user message 中的动态提醒 | 对话流中 | 每次 tool result |

### 1.2 system-reminder 侧信道机制

这是原分析文档**未覆盖**的重要机制。Claude Code 使用 `<system-reminder>` XML 标签在对话流中注入系统级信息，而非修改 system prompt。

**工作方式**：
- 在 tool result 返回后，自动追加 `<system-reminder>` 块
- 内容包括：可用 skill 列表、CLAUDE.md 项目说明、当前日期等
- 模型被训练为将此类标签视为系统级指令

**设计价值**：
- 不需要重建 system prompt 就能动态追加上下文
- 对话进行中可以刷新信息（如 skill 列表变更）
- 与 tool result 绑定，模型在处理工具输出时自然接收到最新上下文

**与传统 system prompt 的区别**：

| 维度 | System Prompt | system-reminder |
|------|--------------|-----------------|
| 注入位置 | API 的 `system` 字段 | messages 数组中的 tool_result content |
| 更新成本 | 需要修改 system 字段（如支持 prompt caching 可能导致缓存失效） | 自然追加到对话流 |
| 时效性 | 会话开头固定 | 随对话进展动态刷新 |
| 遵循强度 | 最高（system 级） | 中高（依赖模型训练） |

### 1.3 工具延迟加载的三层分类

Claude Code 的工具系统实际上分为三类（原文档仅分为两类）：

1. **立即加载工具**（Full Schema）：Bash、Read、Write、Edit、Glob、Grep、WebFetch 等。完整 JSON Schema 直接通过 `tools` API 参数传递。
2. **延迟加载工具**（Name Only）：TaskCreate、TaskUpdate、WebSearch 等。仅在 system prompt 中列出名称和简要描述，需要通过 ToolSearch 解锁完整 schema 后才能使用。
3. **Skill 工具**（Indirect）：通过 Skill tool 调用的能力不直接暴露 schema，而是由 Skill tool 作为代理封装。

**核心设计意图**：延迟加载不仅节省 token，更重要的是**控制模型行为路径**。不给 schema，模型就不会主动使用该工具，从而避免在简单任务中启动重型工作流（如 TaskCreate）。

### 1.4 行为约束的冗余强化策略

Claude Code 在提示词中采用"关键规则多点重复"策略：

- git 安全规则在 Bash 工具说明中详细定义，又在系统提示的 git 操作章节重复
- 文件操作优先级在工具描述（"DO NOT use bash for file operations"）和行为准则中各出现一次
- "CRITICAL"、"IMPORTANT"、"NEVER" 等强约束词大量使用

这种冗余设计牺牲了 token 效率，但提高了指令遵循率。模型在不同上下文中多次看到同一规则，违反的概率降低。

### 1.5 提示词规模与缓存策略

Claude Code 的 system prompt 约 15000-20000 token，加上 CLAUDE.md 和 skill 描述可达 25000+。其策略前提是：

- 200K 上下文模型，system prompt 占比约 12%，可接受
- Anthropic 的 prompt caching 可缓存 system prompt 的稳定部分
- "宁可多给不要少给"——首轮成本高但信息完整，减少后续探测轮次

---

## 二、当前项目现状（基于代码审查）

### 2.1 最关键的架构问题：两条割裂的 Prompt 构建路径

**当前存在两套完全独立的 prompt 构建路径，互不相通，这是最大的技术债。**

**路径 A — bootstrap 路径（生产环境实际运行）**

位于 `crates/nova-app/src/bootstrap.rs`：

```
1. agent_prompt = read_file(".nova/prompts/agent-{id}.md")
   或 config.toml 中内嵌的 system_prompt_template
2. skill_prompt = skill_registry.generate_system_prompt()
   → 所有已加载 skill 的 instructions 全量拼接
3. behavior_guards = 硬编码字符串（"CRITICAL: Action Consistency..."）
4. full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards)
5. → 存入 Session 初始历史（Role::System 消息）
6. → 后续 run_turn() 直接传递历史，不重新构建 prompt
```

**路径 B — SystemPromptBuilder 路径（agent.rs 中定义，未被主流程调用）**

位于 `crates/nova-core/src/prompt.rs` + `crates/nova-core/src/agent.rs`：

```
SystemPromptBuilder::new()
  .base_section("Zero-Nova Agent")              ← 硬编码字符串
  .agent_section("AI Assistant with tool support") ← 硬编码字符串
  .environment_agent()                           ← 静态 "Zero-Nova Agent Environment"
  .skill_section(active_skill_instructions)      ← 仅活跃 skill
  .tool_guidance_section("")                     ← 空
  .workflow_section("")                          ← 空
  .with_tools(&tools)                            ← 工具 schema 格式化
  .history_section("")                           ← 空
  .build()
```

路径 B 的 `build_system_prompt()` 和 `prepare_turn()` 在 `agent.rs` 中完整定义，但 `ConversationService` 调用的是旧的 `run_turn()` 方法，完全不经过路径 B。

**影响**：
- 路径 A 不注入工具 schema（依赖 API 的 tools 参数），路径 B 在 prompt 中注入工具 schema
- 路径 A 全量注入所有 skill，路径 B 仅注入活跃 skill
- 路径 A 不做模板变量替换，路径 B 也没实现替换
- 两条路径产生完全不同的 prompt 内容

### 2.2 各层能力现状对照

| Claude Code 层级 | Zero-Nova 对应 | 实现状态 | 具体问题 |
|------------------|---------------|----------|----------|
| L0 平台身份 | `agent-nova.md` | ⚠️ 部分实现 | 内容相对单薄，缺少输出风格、安全边界等系统级规范 |
| L1 工具能力 | `tools` API 参数 | ✅ 工作正常 | API 层正确传递工具定义；bootstrap 路径不在 prompt 中注入工具描述 |
| L2 Skills 目录 | `generate_system_prompt()` | ⚠️ 有问题 | 全量注入所有 skill 的完整 instructions，不区分 active/inactive |
| L3 项目上下文 | ❌ 不存在 | 无实现 | 没有类似 CLAUDE.md 的项目说明自动加载机制 |
| L4 记忆规则 | ❌ 不存在 | 无实现 | 无跨会话记忆管理，Session 历史全量传递无裁剪 |
| L5 环境快照 | ❌ 不存在 | 无实现 | 没有自动注入 CWD、git、平台等运行时信息 |
| L6 侧信道注入 | ❌ 不存在 | 无实现 | 没有对话流中的动态上下文刷新机制 |

### 2.3 已确认的代码缺陷清单

以下问题通过代码审查确认存在：

| # | 位置 | 严重程度 | 问题描述 |
|---|------|----------|----------|
| B1 | `prompt.rs` build() | 中 | `Low` 优先级过滤条件为恒真式 `section.priority != Low \|\| !section.content.is_empty()`，空检查在前面已排除空 section，所以 Low 永远不被过滤 |
| B2 | `prompt.rs` build() | 低 | sections 之间只用 `\n` 连接，无结构化标题或分隔符，LLM 难以区分各部分 |
| B3 | `bootstrap.rs` | 高 | 直接 `format!()` 拼接三段字符串，完全绕过 `SystemPromptBuilder`，两套路径割裂 |
| B4 | `skill.rs` generate_system_prompt() | 中 | 所有已加载 skill 全量注入，不区分 active/inactive，token 浪费随 skill 数量线性增长 |
| B5 | `agent-nova.md` + bootstrap | 中 | `{{workflow_stage}}`、`{{pending_interaction}}` 占位符无运行时替换逻辑，字面量原样发送给 LLM |
| B6 | `workflow-stages.md` | 低 | 定义了完整的 7 阶段 prompt 模板，但代码中从未读取和使用该文件 |
| B7 | `agent.rs` build_system_prompt() | 高 | 使用硬编码 `"Zero-Nova Agent"` 和 `"AI Assistant with tool support"`，不读取 agent prompt 文件，与 bootstrap 路径完全独立 |
| B8 | `agent.rs` run_turn_with_context() | 高 | 工具执行逻辑缺失——只记录 tool_calls 不执行，usage 统计也未更新 |
| B9 | `skill.rs` policy_from_skill() | 中 | AllowList 模式下先 `always_enabled_tools.clear()`，然后过滤掉 Bash/Read/Write/Edit 不加入 deferred_tools，导致基础工具从可用列表中消失 |

### 2.4 设计与实现的差距

| 设计文档/代码定义 | 预期功能 | 实现状态 |
|------------------|----------|----------|
| `SystemPromptBuilder` 7 个 Section | 分层 prompt 构建 | 定义完整，但未被主流程使用 |
| `prepare_turn()` + `TurnContext` | Turn 前准备（skill 路由、工具裁剪、历史裁剪） | 方法存在但未接入 ConversationService |
| `workflow-stages.md` | 7 阶段工作流 prompt | 文件存在但无加载/注入代码 |
| `turn-router.md` | LLM 辅助意图分类 | 文件存在但对应路由逻辑未确认接入 |
| `SkillInvocationLevel` 三层模型 | Session/Tool/User 三层 skill 路由 | 仅 User 级（`/skill-name`）实现 |
| `{{占位符}}` 模板变量 | 动态 prompt 内容 | 无替换逻辑 |
| `config.toml [gateway.trimmer]` | 历史裁剪 | 配置存在但 trimmer 逻辑未接入 |

---

## 三、增强目标

### 3.1 核心目标

**统一 prompt 构建管道，使 bootstrap 和 agent runtime 共用同一套构建逻辑**，并逐步补齐 Claude Code 已验证有效的分层注入能力。

### 3.2 分阶段目标

#### P0 — 统一与修复（必须先做）

| 编号 | 目标 | 解决的问题 |
|------|------|----------|
| G1 | 统一 prompt 构建管道 | 消除 bootstrap 和 agent.rs 两条割裂路径（B3, B7） |
| G2 | 修复 build() 过滤逻辑 | Low 优先级恒真式 bug（B1） |
| G3 | 修复 AllowList 工具消失 | 基础工具在白名单模式下丢失（B9） |
| G4 | 结构化 section 输出 | section 间添加标题和分隔符（B2） |

#### P1 — 核心增强

| 编号 | 目标 | 对标 Claude Code |
|------|------|----------------|
| G5 | 环境快照注入 | 对应 L5（CWD、git、平台） |
| G6 | 项目上下文加载 | 对应 L3（PROJECT.md 自动加载） |
| G7 | Skill 按需注入 | 改进 L2（仅活跃 skill 全量，其余仅名称） |
| G8 | 模板变量替换 | 解决 B5（占位符实际替换） |

#### P2 — 高级功能

| 编号 | 目标 | 说明 |
|------|------|------|
| G9 | 历史管理策略 | token 预算感知的历史裁剪 |
| G10 | 侧信道注入 | 对话流中的动态上下文刷新 |
| G11 | prepare_turn 接入 | 让 TurnContext 路径成为主流程 |

---

## 四、详细设计

### 4.1 G1 — 统一 Prompt 构建管道

#### 4.1.1 当前流程

```
bootstrap.rs                          agent.rs（未使用）
     │                                     │
  read agent-nova.md                  build_system_prompt()
  + generate_system_prompt()           + SystemPromptBuilder
  + behavior_guards                    + 硬编码字符串
     │                                     │
  format!() 拼接                        builder.build()
     │                                     │
  存入 Session                           （未接入）
```

#### 4.1.2 目标流程

```
bootstrap.rs
     │
  创建 PromptConfig
     │
  SystemPromptBuilder::from_config(config, tools, skills)
     │
  SystemPromptBuilder
     ├── Base          ← agent-{id}.md 文件内容（经模板替换）
     ├── ToolGuidance  ← 行为约束 + 工具优先级指导
     ├── Skill         ← skill_registry.generate_contextual_prompt()
     ├── ProjectContext ← workspace/PROJECT.md（如存在）
     ├── Environment   ← 运行时环境快照
     └── build() → 结构化 prompt
     │
  存入 Session / 或每轮动态构建
```

#### 4.1.3 新增结构体

```rust
// crates/nova-core/src/prompt.rs

/// Prompt 构建所需的完整配置
pub struct PromptConfig {
    pub agent_id: String,
    pub agent_prompt: String,              // 从文件读取的 agent prompt
    pub workspace_path: PathBuf,           // 工作区路径
    pub active_skill: Option<String>,      // 当前活跃 skill id
    pub template_vars: HashMap<String, String>, // 模板变量
    pub environment: EnvironmentSnapshot,  // 环境快照
}

/// 运行时环境快照
pub struct EnvironmentSnapshot {
    pub working_directory: String,
    pub platform: String,
    pub shell: String,
    pub git_branch: Option<String>,
    pub git_status_summary: Option<String>,
    pub recent_commits: Option<String>,
    pub model_id: Option<String>,
    pub current_date: String,
}
```

#### 4.1.4 from_config 统一入口

```rust
impl SystemPromptBuilder {
    pub fn from_config(
        config: &PromptConfig,
        tools: &ToolRegistry,
        skills: &SkillRegistry,
    ) -> Self {
        let mut builder = Self::new();

        // L0: 平台身份（agent prompt 文件，经模板替换）
        let rendered_prompt = TemplateContext::render(
            &config.agent_prompt,
            &config.template_vars,
        );
        builder = builder.base_section(&rendered_prompt);

        // L1: 工具能力（行为约束嵌入此处）
        builder = builder
            .tool_guidance_section(BEHAVIOR_GUARDS)
            .with_tools(tools);

        // L2: Skills（按需注入）
        let skill_prompt = skills.generate_contextual_prompt(
            config.active_skill.as_deref()
        );
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        // L3: 项目上下文
        if let Some(content) = load_project_context(&config.workspace_path) {
            builder = builder.project_context_section(&content);
        }

        // L5: 环境快照
        builder = builder.environment_snapshot(&config.environment);

        builder
    }
}
```

#### 4.1.5 修改 bootstrap.rs

```rust
// 改前：
let full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards);

// 改后：
let env_snapshot = EnvironmentSnapshot::collect().await?;
let prompt_config = PromptConfig {
    agent_id: agent.id.clone(),
    agent_prompt,
    workspace_path: config.workspace_path().to_path_buf(),
    active_skill: None,
    template_vars: HashMap::from([
        ("workflow_stage".into(), "idle".into()),
        ("pending_interaction".into(), "none".into()),
    ]),
    environment: env_snapshot,
};
let full_system_prompt = SystemPromptBuilder::from_config(
    &prompt_config, &tool_registry, &skill_registry
).build();
```

#### 4.1.6 同步修改 agent.rs

`build_system_prompt()` 不再硬编码字符串，改为接收 `PromptConfig` 参数：

```rust
// 改前：
pub fn build_system_prompt(&self, active_skill: Option<&SkillPackage>) -> String {
    let builder = SystemPromptBuilder::new()
        .base_section("Zero-Nova Agent")
        .agent_section("AI Assistant with tool support");
    // ...
}

// 改后：
pub fn build_system_prompt(&self, config: &PromptConfig, skills: &SkillRegistry) -> String {
    SystemPromptBuilder::from_config(config, &self.tools, skills).build()
}
```

### 4.2 G2 — 修复 build() 过滤逻辑

```rust
// crates/nova-core/src/prompt.rs

// 改前：
.filter(|(_, section)| {
    if section.content.is_empty() { return false; }
    section.priority != PromptPriority::Low || !section.content.is_empty()
    // 恒真式：!Low 为 true 或 !empty 为 true（前面已过滤空）
})

// 改后：
.filter(|(_, section)| !section.content.is_empty())
// 简化为仅过滤空 section
// Low 优先级的裁剪由 token budget 机制控制（G9），不在 build() 层面处理
```

### 4.3 G3 — 修复 AllowList 工具消失

```rust
// crates/nova-core/src/skill.rs — policy_from_skill()

// 改前：
ToolPolicy::AllowList(tools) | ToolPolicy::AllowListWithDeferred(tools) => {
    policy.always_enabled_tools.clear();
    for tool in tools {
        if !["Bash", "Read", "Write", "Edit"].contains(&tool.as_str()) {
            policy.deferred_tools.push(tool.clone());
        }
    }
}

// 改后：
ToolPolicy::AllowList(tools) | ToolPolicy::AllowListWithDeferred(tools) => {
    let base_tool_names: HashSet<&str> = ["Bash", "Read", "Write", "Edit"].into_iter().collect();
    // 白名单中的基础工具保留在 always_enabled
    policy.always_enabled_tools = tools.iter()
        .filter(|t| base_tool_names.contains(t.as_str()))
        .cloned()
        .collect();
    // 白名单中的非基础工具放入 deferred
    policy.deferred_tools = tools.iter()
        .filter(|t| !base_tool_names.contains(t.as_str()))
        .cloned()
        .collect();
}
```

### 4.4 G4 — 结构化 Section 输出

```rust
// crates/nova-core/src/prompt.rs

// 新增 SectionName 变体
pub enum SectionName {
    Base,
    Agent,
    Skill,
    ProjectContext,    // 新增
    BehaviorGuards,    // 新增
    Environment,
    Workflow,
    ToolGuidance,
    History,
}

impl SectionName {
    fn heading(&self) -> &str {
        match self {
            Self::Base => "Identity & Role",
            Self::Agent => "Agent Configuration",
            Self::Skill => "Available Skills",
            Self::ProjectContext => "Project Context",
            Self::BehaviorGuards => "Behavior Constraints",
            Self::Environment => "Environment",
            Self::Workflow => "Workflow State",
            Self::ToolGuidance => "Tool Capabilities",
            Self::History => "Conversation Summary",
        }
    }
}

// 修改 build()：
pub fn build(self) -> String {
    self.sections
        .into_iter()
        .filter(|(_, section)| !section.content.is_empty())
        .map(|(name, section)| {
            format!("## {}\n\n{}", name.heading(), section.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}
```

### 4.5 G5 — 环境快照注入

```rust
// crates/nova-core/src/prompt.rs

impl EnvironmentSnapshot {
    /// 采集当前运行环境信息
    pub async fn collect() -> Result<Self> {
        let working_directory = std::env::current_dir()?
            .to_string_lossy().to_string();
        let platform = std::env::consts::OS.to_string();
        let shell = std::env::var("SHELL")
            .or_else(|_| std::env::var("COMSPEC"))
            .unwrap_or_else(|_| "unknown".to_string());

        let git_branch = Self::run_git(&["rev-parse", "--abbrev-ref", "HEAD"]).await.ok();
        let git_status = Self::run_git(&["status", "--short"]).await.ok().map(|s| {
            let count = s.lines().count();
            if count == 0 { "clean".to_string() } else { format!("{} changed files", count) }
        });
        let recent_commits = Self::run_git(&[
            "log", "--oneline", "-5"
        ]).await.ok();

        Ok(Self {
            working_directory,
            platform,
            shell,
            git_branch,
            git_status_summary: git_status,
            recent_commits,
            model_id: None,
            current_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        })
    }

    async fn run_git(args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new("git")
            .args(args)
            .output().await?;
        if output.status.success() {
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        } else {
            anyhow::bail!("git command failed")
        }
    }

    /// 生成 prompt section 文本
    pub fn to_prompt_text(&self) -> String {
        let mut lines = vec![
            format!("Working directory: {}", self.working_directory),
            format!("Platform: {}", self.platform),
            format!("Shell: {}", self.shell),
            format!("Date: {}", self.current_date),
        ];
        if let Some(branch) = &self.git_branch {
            lines.push(format!("Git branch: {}", branch));
        }
        if let Some(status) = &self.git_status_summary {
            lines.push(format!("Git status: {}", status));
        }
        if let Some(commits) = &self.recent_commits {
            lines.push(format!("\nRecent commits:\n{}", commits));
        }
        if let Some(model) = &self.model_id {
            lines.push(format!("Model: {}", model));
        }
        lines.join("\n")
    }
}

// SystemPromptBuilder 方法
impl SystemPromptBuilder {
    pub fn environment_snapshot(mut self, env: &EnvironmentSnapshot) -> Self {
        self.sections.insert(SectionName::Environment, PromptSection {
            content: env.to_prompt_text(),
            priority: PromptPriority::High,
        });
        self
    }
}
```

### 4.6 G6 — 项目上下文加载

支持在工作区放置 `PROJECT.md` 或 `NOVA.md` 作为项目级上下文：

```rust
// crates/nova-core/src/prompt.rs

const PROJECT_CONTEXT_FILES: &[&str] = &["PROJECT.md", "NOVA.md"];

fn load_project_context(workspace: &Path) -> Option<String> {
    for filename in PROJECT_CONTEXT_FILES {
        let path = workspace.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
}

impl SystemPromptBuilder {
    pub fn project_context_section(mut self, content: &str) -> Self {
        self.sections.insert(SectionName::ProjectContext, PromptSection {
            content: content.to_string(),
            priority: PromptPriority::Medium,
        });
        self
    }
}
```

### 4.7 G7 — Skill 按需注入

#### 当前问题

`generate_system_prompt()` 将所有 skill 的完整 instructions 全量拼接，不管是否活跃。当 skill 数量增多时 token 开销线性增长。

#### 目标行为

- **无活跃 skill**：仅注入 skill 名称 + 描述的简短索引（类似 Claude Code 的 skill 目录）
- **有活跃 skill**：注入该 skill 的完整 instructions + 其余 skill 的名称列表

```rust
// crates/nova-core/src/skill.rs

impl SkillRegistry {
    /// 生成上下文感知的 skill prompt（替代 generate_system_prompt）
    pub fn generate_contextual_prompt(&self, active_skill_id: Option<&str>) -> String {
        let packages = self.list_packages();
        if packages.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        // 活跃 skill 完整注入
        if let Some(active_id) = active_skill_id {
            if let Some(pkg) = packages.iter().find(|p| p.slug == active_id) {
                parts.push(format!(
                    "### [ACTIVE] {}\n\n{}\n",
                    pkg.display_name, pkg.instructions
                ));
            }
        }

        // 其余 skill 仅名称+描述
        let other_skills: Vec<String> = packages.iter()
            .filter(|p| active_skill_id.map(|id| id != p.slug).unwrap_or(true))
            .map(|p| format!("- **{}**: {}", p.display_name, p.description))
            .collect();

        if !other_skills.is_empty() {
            parts.push(format!(
                "### Other Available Skills\n\n{}\n\nUse `/skill-<name>` to activate.",
                other_skills.join("\n")
            ));
        }

        parts.join("\n")
    }
}
```

### 4.8 G8 — 模板变量替换

```rust
// crates/nova-core/src/prompt.rs

pub struct TemplateContext;

impl TemplateContext {
    /// 替换 {{key}} 占位符，未匹配的替换为空字符串
    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        // 清理未匹配的占位符
        lazy_static::lazy_static! {
            static ref RE: regex::Regex = regex::Regex::new(
                r"\{\{[a-zA-Z_][a-zA-Z0-9_]*\}\}"
            ).unwrap();
        }
        RE.replace_all(&result, "").to_string()
    }
}
```

---

## 五、实施计划

每个 Phase 有独立的详细设计文档，包含完整的代码变更方案、变更清单、测试计划和风险分析。

### Phase 1 — 统一与修复（P0）

**详细设计**：[`2026-04-25-prompt-phase1-unify-and-fix.md`](./2026-04-25-prompt-phase1-unify-and-fix.md)

| 目标 | 内容 | 涉及文件 |
|------|------|----------|
| G1 — 统一构建管道 | 新增 `PromptConfig`、`from_config()`，重构 bootstrap.rs 和 agent.rs | prompt.rs, bootstrap.rs, agent.rs |
| G2 — 修复 build() 过滤 | 简化 Low 优先级恒真式 bug | prompt.rs |
| G3 — 修复 AllowList 工具消失 | 保留白名单中的基础工具到 always_enabled | skill.rs |
| G4 — 结构化 section 输出 | 添加 `## heading` 标题和 `---` 分隔符 | prompt.rs |

关键决策：Phase 1 **不改变 prompt 语义内容**，仅统一构建路径和修复 bug。Skill 全量注入行为暂时保持不变，确保行为一致性。

### Phase 2 — 核心增强（P1）

**详细设计**：[`2026-04-25-prompt-phase2-core-enhancement.md`](./2026-04-25-prompt-phase2-core-enhancement.md)

| 目标 | 内容 | 涉及文件 |
|------|------|----------|
| G5 — 环境快照注入 | `EnvironmentSnapshot::collect()` 采集 CWD/git/平台信息 | prompt.rs, bootstrap.rs |
| G6 — 项目上下文加载 | 自动加载 PROJECT.md / NOVA.md，支持大小限制和截断 | prompt.rs, config.rs |
| G7 — Skill 按需注入 | `generate_contextual_prompt()` 替代全量注入，节省 75-96% token | skill.rs |
| G8 — 模板变量替换增强 | 正则替换 + 清理模式 + 变量提取 + 预定义变量集 | prompt.rs, bootstrap.rs |

关键决策：`generate_system_prompt()` 保留但标记 `#[deprecated]`，新代码统一使用 `generate_contextual_prompt()`。

### Phase 3 — 高级功能（P2）

**详细设计**：[`2026-04-25-prompt-phase3-advanced-features.md`](./2026-04-25-prompt-phase3-advanced-features.md)

| 目标 | 内容 | 涉及文件 |
|------|------|----------|
| G9 — 历史管理策略 | `HistoryTrimmer` + token 估算 + 预算分配 + 裁剪提示插入 | prompt.rs, agent.rs, config.rs |
| G10 — 侧信道注入 | `SideChannelInjector` 在 tool result 中嵌入 `<system-reminder>` | prompt.rs, agent.rs |
| G11 — prepare_turn 接入 | 修复 `run_turn_with_context()` 工具执行缺陷，切换主流程 | agent.rs, conversation_service.rs |
| 附加 — Workflow 阶段加载 | `WorkflowStagePrompts` 解析 workflow-stages.md 并注入 | prompt.rs |

关键决策：通过 `use_turn_context` 配置开关渐进切换到新路径，降低回归风险。

---

## 六、风险总览

| 风险 | Phase | 影响 | 缓解措施 |
|------|-------|------|----------|
| 统一管道后 prompt 内容变化导致行为回归 | 1 | 高 | Phase 1 不改变语义，对比新旧 prompt 输出 |
| 修改 build() 输出格式影响现有测试 | 1 | 中 | 同步更新测试断言 |
| AllowList 修复改变工具可见性 | 1 | 中 | 新增针对性测试覆盖 |
| git 命令在非 git 环境失败 | 2 | 中 | 所有 git 信息为 `Option<String>`，失败静默跳过 |
| 项目上下文文件非 UTF-8 编码 | 2 | 低 | `read_to_string` 报错时静默跳过 |
| Skill 按需注入后 LLM 不知道如何激活 | 2 | 中 | 索引表包含 `/skill-<name>` 使用提示 |
| Token 估算误差导致裁剪不准 | 3 | 中 | 保守估算（chars/3），后续接入精确 tokenizer |
| 裁剪破坏 tool_use/tool_result 配对 | 3 | 高 | 以完整消息为裁剪单位，保护最近 N 条 |
| 切换 run_turn_with_context 行为回归 | 3 | 高 | `use_turn_context` 配置开关渐进切换 |
| `<system-reminder>` 标签被非 Anthropic 模型忽略 | 3 | 中 | agent prompt 中显式说明标签含义 |

---

## 七、测试用例总览

详细测试用例见各 Phase 文档。以下是跨 Phase 的关键验证点：

### 端到端验证

1. **Phase 1 完成后**：bootstrap 产生的 prompt 包含 `## Identity & Role`、`## Behavior Constraints`、`## Available Skills`，内容与修改前语义等价
2. **Phase 2 完成后**：prompt 新增 `## Environment`（含 git/CWD）、`## Project Context`（如存在 PROJECT.md）；skill section 仅含索引（无活跃 skill 时）
3. **Phase 3 完成后**：长对话自动裁剪历史并插入提示；`run_turn_with_context()` 正确执行工具；tool result 附带 `<system-reminder>` 信息

### 回归验证

每个 Phase 合并后必须通过完整检查周期：

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```
