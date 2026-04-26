# Phase 1：统一与修复（P0）

- 日期：2026-04-25
- 状态：草案
- 优先级：P0 — 后续所有增强的前提
- 主文档：`docs/design/2026-04-25-prompt-architecture-enhancement.md`
- 涉及文件：
  - `crates/nova-core/src/prompt.rs`
  - `crates/nova-core/src/agent.rs`
  - `crates/nova-core/src/skill.rs`
  - `crates/nova-app/src/bootstrap.rs`

---

## 一、目标

| 编号 | 目标 | 解决的问题 |
|------|------|----------|
| G1 | 统一 prompt 构建管道 | 消除 bootstrap.rs 和 agent.rs 两条割裂路径 |
| G2 | 修复 build() 过滤逻辑 | Low 优先级恒真式 bug |
| G3 | 修复 AllowList 工具消失 | 基础工具在白名单模式下丢失 |
| G4 | 结构化 section 输出 | section 间添加标题和分隔符 |

---

## 二、G1 — 统一 Prompt 构建管道

### 2.1 问题描述

当前存在两条完全独立的 prompt 构建路径：

**路径 A — bootstrap.rs（生产环境实际运行）**

```rust
// bootstrap.rs:67
let full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards);
```

- 从 `agent-{id}.md` 文件或 config 内嵌模板读取 agent prompt
- 调用 `skill_registry.generate_system_prompt()` 全量注入所有 skill
- 拼接硬编码的 behavior_guards 字符串
- 结果存入 `AgentDescriptor.system_prompt_template`

**路径 B — agent.rs build_system_prompt()（定义但未使用）**

```rust
// agent.rs:519-546
fn build_system_prompt(&self, ...) -> String {
    let mut builder = SystemPromptBuilder::new();
    builder = builder
        .base_section("Zero-Nova Agent")          // 硬编码
        .agent_section("AI Assistant with tool support");  // 硬编码
    builder = builder.environment_agent();         // 静态 "Zero-Nova Agent Environment"
    // ...
    builder.build()
}
```

- 使用硬编码字符串，不读取 agent prompt 文件
- 通过 `SystemPromptBuilder` 构建，但未被 `ConversationService` 调用

两条路径产生完全不同的 prompt 内容，且路径 B 的 `prepare_turn()` 从未接入主流程。

### 2.2 设计方案

#### 2.2.1 新增 PromptConfig 结构体

在 `prompt.rs` 中新增，作为 prompt 构建的统一输入：

```rust
// crates/nova-core/src/prompt.rs — 新增

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Prompt 构建所需的完整配置。
/// 由 bootstrap / CLI / ConversationService 统一创建。
#[derive(Debug, Clone)]
pub struct PromptConfig {
    /// Agent 标识（用于日志和调试）
    pub agent_id: String,
    /// 从文件加载的 agent prompt 内容（已读取为字符串）
    pub agent_prompt: String,
    /// 工作区路径（用于加载项目上下文文件等）
    pub workspace_path: PathBuf,
    /// 当前活跃的 skill id（如果有）
    pub active_skill: Option<String>,
    /// 模板变量键值对（用于替换 {{key}} 占位符）
    pub template_vars: HashMap<String, String>,
}

impl PromptConfig {
    pub fn new(agent_id: impl Into<String>, agent_prompt: impl Into<String>, workspace_path: PathBuf) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_prompt: agent_prompt.into(),
            workspace_path,
            active_skill: None,
            template_vars: HashMap::new(),
        }
    }

    pub fn with_active_skill(mut self, skill_id: impl Into<String>) -> Self {
        self.active_skill = Some(skill_id.into());
        self
    }

    pub fn with_template_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.template_vars.insert(key.into(), value.into());
        self
    }
}
```

#### 2.2.2 新增 SectionName 变体

```rust
// crates/nova-core/src/prompt.rs — 修改 SectionName 枚举

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionName {
    Base,
    Agent,
    Skill,
    ProjectContext,    // 新增：项目上下文（Phase 2 使用，此处预留）
    BehaviorGuards,    // 新增：行为约束（从硬编码字符串提升为正式 section）
    Environment,
    Workflow,
    ToolGuidance,
    History,
}
```

#### 2.2.3 为 SectionName 实现 heading()

```rust
// crates/nova-core/src/prompt.rs — 新增 impl

impl SectionName {
    /// 返回该 section 在最终 prompt 中的标题。
    pub fn heading(&self) -> &str {
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
```

#### 2.2.4 新增 behavior_guards_section() 方法

将 bootstrap.rs 中硬编码的 behavior_guards 提升为 `SystemPromptBuilder` 的正式方法：

```rust
// crates/nova-core/src/prompt.rs — 新增方法

/// 内置行为约束文本。
const BEHAVIOR_GUARDS: &str = r#"
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
"#;

impl SystemPromptBuilder {
    /// 添加行为约束 section。
    pub fn behavior_guards_section(self) -> Self {
        self.add_section(SectionName::BehaviorGuards, BEHAVIOR_GUARDS.trim(), PromptPriority::High)
    }

    /// 添加项目上下文 section（Phase 2 使用）。
    pub fn project_context_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::ProjectContext, content, PromptPriority::Medium)
    }
}
```

#### 2.2.5 新增 from_config() 统一入口

```rust
// crates/nova-core/src/prompt.rs — 新增方法

impl SystemPromptBuilder {
    /// 从配置创建完整的 system prompt builder。
    ///
    /// 这是所有路径（bootstrap、CLI、prepare_turn）的统一入口。
    /// 构建的 section 顺序：
    ///   Base → BehaviorGuards → Skill → ToolGuidance → Environment
    pub fn from_config(
        config: &PromptConfig,
        skills: &crate::skill::SkillRegistry,
    ) -> Self {
        let mut builder = Self::new();

        // L0: 平台身份（agent prompt 文件内容）
        // 如果有模板变量，进行替换
        let rendered_prompt = if config.template_vars.is_empty() {
            config.agent_prompt.clone()
        } else {
            TemplateContext::render(&config.agent_prompt, &config.template_vars)
        };
        if !rendered_prompt.is_empty() {
            builder = builder.base_section(&rendered_prompt);
        }

        // L1: 行为约束
        builder = builder.behavior_guards_section();

        // L2: Skills（按需注入 — Phase 2 中由 generate_contextual_prompt 替换）
        // Phase 1 暂时保持全量注入以确保行为一致
        let skill_prompt = skills.generate_system_prompt();
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        builder
    }
}
```

#### 2.2.6 新增 TemplateContext（简单模板替换）

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 简单的 {{key}} 模板变量替换。
pub struct TemplateContext;

impl TemplateContext {
    /// 替换模板中的 {{key}} 占位符。
    /// 未匹配的占位符保持原样（Phase 1 不做清理，Phase 2 中改为清理）。
    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }
}
```

#### 2.2.7 修改 bootstrap.rs

```rust
// crates/nova-app/src/bootstrap.rs — 修改 build_application 函数

// === 改前 (行 28, 59-67, 74) ===
let skill_prompt = skill_registry.generate_system_prompt();
// ...
let behavior_guards = r#"..."#;
let full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards);
// ...
system_prompt_template: full_system_prompt,

// === 改后 ===
// 删除 skill_prompt 变量（不再在此处调用 generate_system_prompt）
// 删除 behavior_guards 硬编码字符串

// 在 agent 循环内部：
let prompt_config = PromptConfig::new(
    agent.id.clone(),
    agent_prompt,
    config.workspace.clone(),
);
let full_system_prompt = SystemPromptBuilder::from_config(
    &prompt_config,
    &skill_registry,
).build();
// ...
system_prompt_template: full_system_prompt,
```

完整修改后的 bootstrap.rs agent 循环代码：

```rust
for agent in &config.gateway.agents {
    let prompt_file = format!("agent-{}.md", agent.id);
    let prompt_path = config.prompts_dir().join(&prompt_file);
    let agent_prompt = match &agent.system_prompt_template {
        Some(prompt) => prompt.clone(),
        None => match tokio::fs::read_to_string(&prompt_path).await {
            Ok(content) => content,
            Err(e) => {
                log::warn!("Failed to read prompt file {:?}: {}", prompt_path, e);
                String::new()
            }
        },
    };

    // 统一通过 SystemPromptBuilder 构建
    let prompt_config = PromptConfig::new(
        agent.id.clone(),
        agent_prompt,
        config.workspace.clone(),
    );
    let full_system_prompt = SystemPromptBuilder::from_config(
        &prompt_config,
        &skill_registry,
    ).build();

    agents.push(AgentDescriptor {
        id: agent.id.clone(),
        display_name: agent.display_name.clone(),
        description: agent.description.clone(),
        aliases: agent.aliases.clone(),
        system_prompt_template: full_system_prompt,
        tool_whitelist: agent.tool_whitelist.clone(),
        model_config: agent.model_config.clone(),
    });
}
```

注意：`let skill_prompt = skill_registry.generate_system_prompt();`（行 28）不再需要，可以删除。但 `skill_registry` 仍需要在循环前创建并传入 `from_config`。

#### 2.2.8 同步修改 agent.rs build_system_prompt()

```rust
// crates/nova-core/src/agent.rs — 修改 build_system_prompt 方法

// === 改前 (行 514-546) ===
fn build_system_prompt(
    &self,
    _capability_policy: &CapabilityPolicy,
    active_skill: &Option<ActiveSkillState>,
) -> String {
    let mut builder = crate::prompt::SystemPromptBuilder::new();
    builder = builder
        .base_section("Zero-Nova Agent")
        .agent_section("AI Assistant with tool support");
    // ...
}

// === 改后 ===
/// 构建系统提示词。
///
/// 接收 PromptConfig 参数，通过 SystemPromptBuilder::from_config 统一构建。
fn build_system_prompt(
    &self,
    config: &crate::prompt::PromptConfig,
) -> String {
    let skills = self.skill_registry.as_ref()
        .map(|sr| sr.as_ref())
        .unwrap_or(&EMPTY_SKILL_REGISTRY);

    crate::prompt::SystemPromptBuilder::from_config(config, skills)
        .with_tools(&self.tools)
        .build()
}

// 在 agent.rs 文件顶部或模块内部添加空 registry 懒静态：
lazy_static::lazy_static! {
    static ref EMPTY_SKILL_REGISTRY: crate::skill::SkillRegistry = crate::skill::SkillRegistry::new();
}
```

注意：如果不希望引入 `lazy_static`，可以改为在方法内构造临时空 registry：

```rust
fn build_system_prompt(&self, config: &crate::prompt::PromptConfig) -> String {
    let empty = crate::skill::SkillRegistry::new();
    let skills = self.skill_registry.as_ref()
        .map(|sr| sr.as_ref())
        .unwrap_or(&empty);
    crate::prompt::SystemPromptBuilder::from_config(config, skills)
        .with_tools(&self.tools)
        .build()
}
```

#### 2.2.9 同步修改 prepare_turn()

```rust
// crates/nova-core/src/agent.rs — 修改 prepare_turn 方法（行 359-394）

pub fn prepare_turn(
    &self,
    input: &str,
    current_history: Arc<Vec<Message>>,
    prompt_config: &crate::prompt::PromptConfig,
) -> Result<TurnContext> {
    // 1. 决定 active skill
    let active_skill = self.decide_active_skill(input, &current_history)?;

    // 2. 根据 active skill 生成 capability policy
    let capability_policy = if let Some(ref as2) = active_skill {
        if let Some(ref sr) = self.skill_registry {
            sr.policy_from_skill(&as2.skill_id)
        } else {
            CapabilityPolicy::default()
        }
    } else {
        CapabilityPolicy::default()
    };

    // 3. 构建 system prompt — 通过统一入口
    let mut config = prompt_config.clone();
    if let Some(ref skill) = active_skill {
        config.active_skill = Some(skill.skill_id.clone());
    }
    let system_prompt = self.build_system_prompt(&config);

    // 4. 过滤工具定义
    let tool_definitions = self.filter_tool_definitions(&capability_policy, &active_skill);

    // 5. 裁剪历史
    let history = self.trim_history(&current_history, &active_skill)?;

    Ok(TurnContext {
        system_prompt,
        tool_definitions,
        history,
        active_skill,
        capability_policy,
        skill_tool_enabled: true,
        max_tokens: self.config.max_tokens,
        iteration_budget: self.config.max_iterations,
    })
}
```

### 2.3 新增 import 依赖

```rust
// crates/nova-app/src/bootstrap.rs — 新增 import
use nova_core::prompt::{PromptConfig, SystemPromptBuilder};
```

### 2.4 行为一致性保证

Phase 1 的核心约束是**不改变最终 prompt 的语义内容**，仅改变构建路径。为此：

- `from_config()` 中 skill 注入暂时保持调用 `generate_system_prompt()`（全量）
- `BEHAVIOR_GUARDS` 常量的文本与 bootstrap.rs 中硬编码字符串**完全一致**
- 模板变量替换作为可选功能，不传入变量时行为与原来完全一致

验证方式：在修改前后分别打印 `full_system_prompt` 的内容，diff 对比确认语义等价（仅格式可能有差异）。

---

## 三、G2 — 修复 build() 过滤逻辑

### 3.1 问题描述

`prompt.rs:227-234` 中的过滤逻辑：

```rust
.filter(|(_, section)| {
    // 跳过空内容
    if section.content.is_empty() {
        return false;
    }
    // 低优先级 section 仅在非空时包含
    section.priority != PromptPriority::Low || !section.content.is_empty()
})
```

第二个条件 `section.priority != PromptPriority::Low || !section.content.is_empty()` 是恒真式：
- 如果 `priority != Low`，则 `true || _` = `true`
- 如果 `priority == Low`，则 `false || !section.content.is_empty()`，但前面已经排除了空内容，所以 `!section.content.is_empty()` 也是 `true`

结果：`Low` 优先级 section 永远不被过滤。

### 3.2 修复方案

简化为仅过滤空 section。`Low` 优先级的裁剪由未来的 token budget 机制控制（Phase 3 G9），不在 `build()` 层面处理。

```rust
// crates/nova-core/src/prompt.rs — 修改 build() 方法（行 224-239）

pub fn build(&self) -> String {
    self.sections
        .iter()
        .filter(|(_, section)| !section.content.is_empty())
        .map(|(name, section)| {
            format!("## {}\n\n{}", name.heading(), section.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}
```

这个修改同时完成了 G4（结构化输出），因为 `map` 中使用了 `name.heading()` 添加标题，并用 `---` 作为 section 分隔符。

### 3.3 对现有测试的影响

现有测试需要更新以适应新的输出格式：

```rust
// 改前断言：
assert!(result.contains("Base content"));

// 改后断言（包含 heading）：
assert!(result.contains("## Identity & Role\n\nBase content"));
```

需要更新的测试：
- `section_with_content_is_included`
- `section_order_is_preserved`（需调整 sections 字段访问方式）

---

## 四、G3 — 修复 AllowList 工具消失

### 4.1 问题描述

`skill.rs:555-572` 中 `policy_from_skill()` 的 AllowList 处理逻辑：

```rust
ToolPolicy::AllowList(tools) => {
    policy.always_enabled_tools.clear();   // 清空基础工具
    for tool in tools {
        if !["Bash", "Read", "Write", "Edit"].contains(&tool.as_str()) {
            policy.deferred_tools.push(tool.clone());
        }
        // 注意：白名单中的 Bash/Read/Write/Edit 被跳过，不加入任何列表
    }
}
```

当白名单包含 `"Bash"` 时：
1. `always_enabled_tools` 被清空 → Bash 从 always_enabled 消失
2. if 条件过滤掉 Bash → Bash 不进入 deferred_tools
3. 结果：Bash 完全不可用

### 4.2 修复方案

```rust
// crates/nova-core/src/skill.rs — 修改 policy_from_skill()（行 551-573）

ToolPolicy::AllowList(tools) | ToolPolicy::AllowListWithDeferred(tools) => {
    let base_tool_names: std::collections::HashSet<&str> =
        ["Bash", "Read", "Write", "Edit"].iter().cloned().collect();

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

    // AllowListWithDeferred 保留 ToolSearch
    if matches!(&pkg.tool_policy, ToolPolicy::AllowListWithDeferred(_)) {
        policy.tool_search_enabled = true;
    } else {
        policy.tool_search_enabled = false;
    }
}
```

### 4.3 新增测试

```rust
// crates/nova-core/src/skill.rs — 新增测试

#[test]
fn policy_from_skill_allow_list_preserves_base_tools() {
    let mut registry = SkillRegistry::new();
    registry.packages.push(SkillPackage {
        id: "test".to_string(),
        slug: "test".to_string(),
        display_name: "Test".to_string(),
        description: "test".to_string(),
        instructions: "test".to_string(),
        tool_policy: ToolPolicy::AllowList(vec![
            "Bash".to_string(),
            "Read".to_string(),
            "CustomTool".to_string(),
        ]),
        sticky: false,
        aliases: vec![],
        examples: vec![],
        source_path: PathBuf::from("test"),
        compat_mode: false,
    });

    let policy = registry.policy_from_skill("test");
    // 基础工具应保留在 always_enabled
    assert!(policy.always_enabled_tools.contains(&"Bash".to_string()));
    assert!(policy.always_enabled_tools.contains(&"Read".to_string()));
    // Write 和 Edit 不在白名单中，不应出现
    assert!(!policy.always_enabled_tools.contains(&"Write".to_string()));
    // 非基础工具应在 deferred
    assert!(policy.deferred_tools.contains(&"CustomTool".to_string()));
}

#[test]
fn policy_from_skill_allow_list_empty_keeps_no_base_tools() {
    let mut registry = SkillRegistry::new();
    registry.packages.push(SkillPackage {
        id: "test".to_string(),
        slug: "test".to_string(),
        display_name: "Test".to_string(),
        description: "test".to_string(),
        instructions: "test".to_string(),
        tool_policy: ToolPolicy::AllowList(vec![
            "CustomTool".to_string(),
        ]),
        sticky: false,
        aliases: vec![],
        examples: vec![],
        source_path: PathBuf::from("test"),
        compat_mode: false,
    });

    let policy = registry.policy_from_skill("test");
    // 白名单不含基础工具 → always_enabled 应为空
    assert!(policy.always_enabled_tools.is_empty());
}
```

---

## 五、G4 — 结构化 Section 输出

### 5.1 问题描述

`prompt.rs:235-238` 中 `build()` 的输出：

```rust
.map(|(_, section)| &section.content)
.cloned()
.collect::<Vec<_>>()
.join("\n")
```

sections 之间仅用 `\n` 连接，无标题、无分隔符。LLM 收到的是一段连续文本，难以区分各部分的来源和职责。

### 5.2 修改方案

已在 G2 的修复中一并完成。修改后的 `build()` 方法为每个 section 添加 `## heading` 标题，并用 `\n\n---\n\n` 分隔。

输出格式示例：

```
## Identity & Role

你是 Nova，Zero-Nova 系统的默认通用助手。
...

---

## Behavior Constraints

- You are a physical entity with real-world capabilities.
...

---

## Available Skills

# Available Skills

## Skill: Skill Creator
...

---

## Tool Capabilities

## Bash
...
```

### 5.3 注意事项

- 工具描述（`with_tools` 注入的内容）内部已经使用了 `## {name}` 格式的标题。与 section heading 混合后会出现 `## Tool Capabilities` 下嵌套 `## Bash` 的情况。这是可接受的，因为 LLM 主要看内容而非 Markdown 层级。
- 如果希望避免层级冲突，可以在未来将工具描述的内部格式从 `##` 改为 `###`。但这不在 Phase 1 范围内。

---

## 六、完整变更清单

| 文件 | 变更类型 | 变更说明 |
|------|----------|----------|
| `prompt.rs` | 修改 | 新增 `SectionName::ProjectContext`、`SectionName::BehaviorGuards` 变体 |
| `prompt.rs` | 新增 | `SectionName::heading()` 方法 |
| `prompt.rs` | 修改 | `build()` 方法：简化过滤 + 结构化输出 |
| `prompt.rs` | 新增 | `BEHAVIOR_GUARDS` 常量 |
| `prompt.rs` | 新增 | `behavior_guards_section()` 方法 |
| `prompt.rs` | 新增 | `project_context_section()` 方法 |
| `prompt.rs` | 新增 | `PromptConfig` 结构体 |
| `prompt.rs` | 新增 | `TemplateContext` 结构体和 `render()` 方法 |
| `prompt.rs` | 新增 | `SystemPromptBuilder::from_config()` 方法 |
| `prompt.rs` | 修改 | 更新现有测试以适应新输出格式 |
| `prompt.rs` | 新增 | `from_config` 相关测试 |
| `bootstrap.rs` | 修改 | 删除 `format!()` 拼接，改用 `SystemPromptBuilder::from_config()` |
| `bootstrap.rs` | 修改 | 删除 `behavior_guards` 硬编码字符串 |
| `bootstrap.rs` | 修改 | 删除行 28 的 `skill_prompt` 变量（移入 from_config 内部） |
| `bootstrap.rs` | 新增 | `use nova_core::prompt::{PromptConfig, SystemPromptBuilder};` |
| `agent.rs` | 修改 | `build_system_prompt()` 签名改为接收 `&PromptConfig` |
| `agent.rs` | 修改 | `prepare_turn()` 签名新增 `prompt_config` 参数 |
| `skill.rs` | 修改 | `policy_from_skill()` 的 AllowList 处理逻辑 |
| `skill.rs` | 新增 | AllowList 相关测试 |

---

## 七、测试计划

### 7.1 单元测试

| 测试 | 文件 | 说明 |
|------|------|------|
| `build_empty_produces_empty` | prompt.rs | 空 builder 产生空字符串 |
| `build_includes_heading` | prompt.rs | 非空 section 输出包含 `## {heading}` |
| `build_separates_with_divider` | prompt.rs | 多个 section 之间有 `---` 分隔 |
| `build_skips_empty_sections` | prompt.rs | 空内容的 section 不输出 |
| `from_config_includes_base_and_guards` | prompt.rs | from_config 产生的 prompt 包含 agent prompt 和行为约束 |
| `from_config_includes_skills` | prompt.rs | from_config 产生的 prompt 包含 skill 内容 |
| `from_config_empty_agent_prompt` | prompt.rs | agent_prompt 为空时 Base section 不输出 |
| `template_render_replaces_vars` | prompt.rs | 模板变量正确替换 |
| `template_render_preserves_unmatched` | prompt.rs | 未匹配的占位符保持原样（Phase 1 行为） |
| `policy_allow_list_preserves_base_tools` | skill.rs | AllowList 模式下白名单中的基础工具保留 |
| `policy_allow_list_deferred_non_base` | skill.rs | AllowList 模式下非基础工具进入 deferred |

### 7.2 集成测试

| 测试 | 说明 |
|------|------|
| bootstrap 生成的 prompt 包含 `## Identity & Role` | 确认结构化输出正常 |
| bootstrap 生成的 prompt 包含 `## Behavior Constraints` | 确认行为约束被注入 |
| bootstrap 生成的 prompt 包含 skill 内容 | 确认 skill 全量注入（Phase 1 行为） |
| 新旧 prompt 语义等价 diff | 对比修改前后的 prompt 输出 |

### 7.3 运行验证

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```

---

## 八、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| prompt 格式变化导致 LLM 行为差异 | 高 | 对比新旧 prompt，确保语义等价；增加 `## heading` 不影响 LLM 理解 |
| `from_config` 引入新 import 导致编译问题 | 低 | `PromptConfig` 在 nova-core 内部定义，bootstrap.rs 通过 nova-core re-export 使用 |
| 修改 `prepare_turn` 签名可能影响现有调用方 | 低 | `prepare_turn` 当前未被任何代码调用，修改签名无破坏性 |
| `build()` 输出格式变化影响现有测试 | 中 | 同步更新所有测试断言 |
