# Phase 2：核心增强（P1）

- 日期：2026-04-25
- 状态：草案
- 优先级：P1 — 依赖 Phase 1 完成
- 前置条件：Phase 1（统一构建管道、修复已知 bug）已合并
- 主文档：`docs/design/2026-04-25-prompt-architecture-enhancement.md`
- 涉及文件：
  - `crates/nova-core/src/prompt.rs`
  - `crates/nova-core/src/skill.rs`
  - `crates/nova-core/src/config.rs`
  - `crates/nova-app/src/bootstrap.rs`

---

## 一、目标

| 编号 | 目标 | 对标 Claude Code | 当前差距 |
|------|------|----------------|----------|
| G5 | 环境快照注入 | L5 — CWD、git、平台信息 | 完全缺失 |
| G6 | 项目上下文加载 | L3 — CLAUDE.md 全量注入 | 完全缺失 |
| G7 | Skill 按需注入 | L2 — skill 列表 + 活跃 skill 完整注入 | 当前全量注入所有 skill |
| G8 | 模板变量替换增强 | 占位符运行时替换 | 占位符原样发送给 LLM |

---

## 二、G5 — 环境快照注入

### 2.1 问题描述

当前 `SystemPromptBuilder` 的 `Environment` section 仅注入静态字符串 `"Zero-Nova Agent Environment"`（`prompt.rs:112-118`）。没有 CWD、git 分支、平台、模型等动态运行时信息。

对比 Claude Code 的做法：每次会话开头自动注入 git branch、git status、最近提交、CWD、平台、shell 类型、模型版本等信息。这让 agent 在首轮就知道自己在什么环境中工作，减少了无意义的探测命令（如 `pwd`、`git branch`）。

### 2.2 设计方案

#### 2.2.1 新增 EnvironmentSnapshot 结构体

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 运行时环境快照，在会话创建时采集一次。
#[derive(Debug, Clone, Default)]
pub struct EnvironmentSnapshot {
    /// 当前工作目录
    pub working_directory: String,
    /// 操作系统平台
    pub platform: String,
    /// Shell 类型
    pub shell: String,
    /// Git 当前分支（非 git 目录时为 None）
    pub git_branch: Option<String>,
    /// Git 状态摘要（如 "3 changed files" 或 "clean"）
    pub git_status_summary: Option<String>,
    /// 最近提交摘要（oneline 格式，最多 5 条）
    pub recent_commits: Option<String>,
    /// 当前使用的模型 ID
    pub model_id: Option<String>,
    /// 当前日期
    pub current_date: String,
}
```

#### 2.2.2 实现 collect() 方法

```rust
// crates/nova-core/src/prompt.rs — 新增

impl EnvironmentSnapshot {
    /// 采集当前运行环境信息。
    ///
    /// git 命令失败时（非 git 目录或无 git 可执行文件）静默跳过，
    /// 确保在任何环境下都能正常工作。
    pub async fn collect() -> Self {
        let working_directory = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let platform = std::env::consts::OS.to_string();

        let shell = std::env::var("SHELL")
            .or_else(|_| std::env::var("COMSPEC"))
            .unwrap_or_else(|_| "unknown".to_string());

        let git_branch = Self::run_git(&["rev-parse", "--abbrev-ref", "HEAD"]).await;

        let git_status_summary = Self::run_git(&["status", "--short"]).await.map(|s| {
            let count = s.lines().filter(|l| !l.is_empty()).count();
            if count == 0 {
                "clean".to_string()
            } else {
                format!("{} changed files", count)
            }
        });

        let recent_commits = Self::run_git(&["log", "--oneline", "-5"]).await;

        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();

        Self {
            working_directory,
            platform,
            shell,
            git_branch,
            git_status_summary,
            recent_commits,
            model_id: None, // 由调用方设置
            current_date,
        }
    }

    /// 运行 git 命令并返回 stdout 输出。
    /// 失败时返回 None（不报错）。
    async fn run_git(args: &[&str]) -> Option<String> {
        let result = tokio::process::Command::new("git")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        match result {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() { None } else { Some(text) }
            }
            _ => None,
        }
    }

    /// 生成 prompt section 文本。
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
            lines.push(String::new()); // 空行分隔
            lines.push("Recent commits:".to_string());
            lines.push(commits.clone());
        }
        if let Some(model) = &self.model_id {
            lines.push(format!("Model: {}", model));
        }

        lines.join("\n")
    }
}
```

#### 2.2.3 新增 environment_snapshot() builder 方法

```rust
// crates/nova-core/src/prompt.rs — 新增方法

impl SystemPromptBuilder {
    /// 添加环境快照 section。
    pub fn environment_snapshot(self, env: &EnvironmentSnapshot) -> Self {
        self.add_section(
            SectionName::Environment,
            env.to_prompt_text(),
            PromptPriority::High,
        )
    }
}
```

#### 2.2.4 修改 PromptConfig 添加 environment 字段

```rust
// crates/nova-core/src/prompt.rs — 修改 PromptConfig

#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub agent_id: String,
    pub agent_prompt: String,
    pub workspace_path: PathBuf,
    pub active_skill: Option<String>,
    pub template_vars: HashMap<String, String>,
    pub environment: Option<EnvironmentSnapshot>,  // 新增
}

impl PromptConfig {
    pub fn new(agent_id: impl Into<String>, agent_prompt: impl Into<String>, workspace_path: PathBuf) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_prompt: agent_prompt.into(),
            workspace_path,
            active_skill: None,
            template_vars: HashMap::new(),
            environment: None,  // 新增
        }
    }

    pub fn with_environment(mut self, env: EnvironmentSnapshot) -> Self {
        self.environment = Some(env);
        self
    }
}
```

#### 2.2.5 修改 from_config() 注入环境快照

```rust
// crates/nova-core/src/prompt.rs — 修改 from_config()

impl SystemPromptBuilder {
    pub fn from_config(
        config: &PromptConfig,
        skills: &crate::skill::SkillRegistry,
    ) -> Self {
        let mut builder = Self::new();

        // L0: 平台身份
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

        // L2: Skills
        let skill_prompt = skills.generate_contextual_prompt(  // G7 变更
            config.active_skill.as_deref()
        );
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        // L3: 项目上下文（G6）
        if let Some(content) = load_project_context(&config.workspace_path) {
            builder = builder.project_context_section(&content);
        }

        // L5: 环境快照（G5 — 新增）
        if let Some(env) = &config.environment {
            builder = builder.environment_snapshot(env);
        }

        builder
    }
}
```

#### 2.2.6 修改 bootstrap.rs 采集环境

```rust
// crates/nova-app/src/bootstrap.rs — 修改 build_application

// 在 agent 循环之前采集一次环境快照
let env_snapshot = EnvironmentSnapshot::collect().await;
let env_snapshot = {
    let mut e = env_snapshot;
    e.model_id = Some(config.llm.model_config.model.clone());
    e
};

// 在 agent 循环内部使用：
let prompt_config = PromptConfig::new(
    agent.id.clone(),
    agent_prompt,
    config.workspace.clone(),
).with_environment(env_snapshot.clone());
```

### 2.3 依赖变更

需要确认 `chrono` crate 是否已在 workspace 依赖中。如果没有：

```toml
# Cargo.toml [workspace.dependencies]
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

`tokio::process::Command` 已通过 tokio 的 `process` feature 支持（当前项目已使用 full features）。

### 2.4 输出示例

```
## Environment

Working directory: D:\git\zero-nova
Platform: windows
Shell: C:\Windows\System32\cmd.exe
Date: 2026-04-25
Git branch: main
Git status: 3 changed files

Recent commits:
d23b110 update
af67da7 update
bac9e91 update
2e595c7 update
acfdbbf update
Model: gpt-oss-120b
```

---

## 三、G6 — 项目上下文加载

### 3.1 问题描述

Claude Code 在每次请求中通过 `<system-reminder>` 注入 CLAUDE.md 的完整内容，让 agent 始终带着项目背景工作。当前项目没有类似机制。

### 3.2 设计方案

#### 3.2.1 项目上下文文件约定

支持两个文件名（按优先级查找，找到即止）：

1. `PROJECT.md` — 项目级说明（推荐）
2. `NOVA.md` — 替代名称

查找路径：`{workspace_path}` 下直接查找。

#### 3.2.2 加载函数

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 项目上下文文件名（按优先级排列）
const PROJECT_CONTEXT_FILES: &[&str] = &["PROJECT.md", "NOVA.md"];

/// 从工作区加载项目上下文文件。
///
/// 按优先级查找 PROJECT.md → NOVA.md，找到第一个非空文件即返回。
/// 所有文件都不存在或为空时返回 None。
fn load_project_context(workspace: &Path) -> Option<String> {
    for filename in PROJECT_CONTEXT_FILES {
        let path = workspace.join(filename);
        match std::fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                log::info!("Loaded project context from {:?}", path);
                return Some(content);
            }
            Ok(_) => {
                log::debug!("Project context file {:?} is empty, skipping", path);
            }
            Err(_) => {
                // 文件不存在，静默跳过
            }
        }
    }
    None
}
```

#### 3.2.3 注入方式

已在 G5 的 `from_config()` 修改中包含。项目上下文通过 `project_context_section()` 注入到 `SectionName::ProjectContext`。

#### 3.2.4 大小限制

项目上下文文件可能很大。设置一个合理的上限，超出时截断并添加提示：

```rust
/// 项目上下文最大字符数（约 4000 token）
const MAX_PROJECT_CONTEXT_CHARS: usize = 16000;

fn load_project_context(workspace: &Path) -> Option<String> {
    for filename in PROJECT_CONTEXT_FILES {
        let path = workspace.join(filename);
        match std::fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                log::info!("Loaded project context from {:?} ({} chars)", path, content.len());
                if content.len() > MAX_PROJECT_CONTEXT_CHARS {
                    let truncated = &content[..MAX_PROJECT_CONTEXT_CHARS];
                    // 在最后一个完整行处截断
                    let last_newline = truncated.rfind('\n').unwrap_or(MAX_PROJECT_CONTEXT_CHARS);
                    let mut result = truncated[..last_newline].to_string();
                    result.push_str("\n\n[... truncated due to size limit ...]");
                    return Some(result);
                }
                return Some(content);
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    None
}
```

#### 3.2.5 输出示例

```
## Project Context

# Zero-Nova

Zero-Nova 是一个 AI Agent 框架，运行时有三层架构...

## 构建命令

cargo build --workspace --release
...
```

### 3.3 配置选项（可选）

在 `config.toml` 中添加可选的项目上下文路径配置：

```toml
[tool]
# 自定义项目上下文文件路径（默认查找 PROJECT.md / NOVA.md）
project_context_file = "docs/project-context.md"
```

```rust
// crates/nova-core/src/config.rs — ToolConfig 新增字段

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    pub skills_dir: Option<String>,
    #[serde(default)]
    pub prompts_dir: Option<String>,
    #[serde(default)]
    pub default_policy: Option<String>,
    /// 自定义项目上下文文件路径。
    /// 如果设置，优先使用此路径而非默认的 PROJECT.md / NOVA.md。
    #[serde(default)]
    pub project_context_file: Option<String>,  // 新增
}
```

加载逻辑更新：

```rust
fn load_project_context_with_config(workspace: &Path, config_path: Option<&str>) -> Option<String> {
    // 如果配置了自定义路径，优先使用
    if let Some(custom) = config_path {
        let path = if Path::new(custom).is_absolute() {
            PathBuf::from(custom)
        } else {
            workspace.join(custom)
        };
        match std::fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                log::info!("Loaded custom project context from {:?}", path);
                return Some(content);
            }
            Ok(_) => {
                log::warn!("Custom project context file {:?} is empty", path);
            }
            Err(e) => {
                log::warn!("Failed to read custom project context file {:?}: {}", path, e);
            }
        }
    }

    // 回退到默认查找
    load_project_context(workspace)
}
```

---

## 四、G7 — Skill 按需注入

### 4.1 问题描述

当前 `SkillRegistry::generate_system_prompt()`（`skill.rs:502-531`）将所有已加载 skill 的完整 `instructions` 全量拼接到 system prompt 中，不区分是否活跃。

对比 Claude Code：
- 仅暴露 skill 名称和触发条件描述（作为目录）
- 不注入任何 skill 的完整 instructions 到 system prompt
- Skill 内容通过 Skill tool 调用时才加载

当 skill 数量增多时，全量注入会导致 system prompt 膨胀。

### 4.2 设计方案

#### 4.2.1 新增 generate_contextual_prompt() 方法

```rust
// crates/nova-core/src/skill.rs — 新增方法

impl SkillRegistry {
    /// 生成上下文感知的 skill prompt。
    ///
    /// - 无 active skill 时：仅输出 skill 名称 + 描述的索引表
    /// - 有 active skill 时：输出该 skill 的完整 instructions + 其余 skill 的名称列表
    ///
    /// 替代 `generate_system_prompt()` 的全量注入。
    pub fn generate_contextual_prompt(&self, active_skill_id: Option<&str>) -> String {
        if self.packages.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        // 活跃 skill：完整注入 instructions
        if let Some(active_id) = active_skill_id {
            if let Some(pkg) = self.find_package_by_id(active_id) {
                parts.push(format!(
                    "### Active Skill: {}\n\n{}\n",
                    pkg.display_name,
                    pkg.instructions,
                ));
            }
        }

        // 其余 skill：仅名称 + 描述
        let other_skills: Vec<String> = self.packages.iter()
            .filter(|p| {
                active_skill_id
                    .map(|id| id != p.id && id != p.slug)
                    .unwrap_or(true)
            })
            .map(|p| {
                let aliases = if p.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (aliases: {})", p.aliases.join(", "))
                };
                format!("- **{}**{}: {}", p.display_name, aliases, p.description)
            })
            .collect();

        if !other_skills.is_empty() {
            let header = if active_skill_id.is_some() {
                "### Other Available Skills"
            } else {
                "### Available Skills"
            };
            parts.push(format!(
                "{}\n\n{}\n\nUse `/skill-<name>` to activate a skill.",
                header,
                other_skills.join("\n"),
            ));
        }

        parts.join("\n\n")
    }
}
```

#### 4.2.2 保留 generate_system_prompt() 作为兼容

不删除 `generate_system_prompt()`，但添加 `#[deprecated]` 标记：

```rust
/// 生成旧格式的整包 system prompt（向后兼容）。
///
/// 请改用 `generate_contextual_prompt()` 以减少 token 消耗。
#[deprecated(note = "Use generate_contextual_prompt() instead")]
pub fn generate_system_prompt(&self) -> String {
    // ... 原有逻辑保持不变
}
```

#### 4.2.3 修改 from_config() 使用新方法

Phase 1 中 `from_config()` 调用 `generate_system_prompt()`，Phase 2 改为 `generate_contextual_prompt()`：

```rust
// from_config() 内：
// Phase 1:
// let skill_prompt = skills.generate_system_prompt();

// Phase 2:
let skill_prompt = skills.generate_contextual_prompt(
    config.active_skill.as_deref()
);
```

### 4.3 输出示例

**无活跃 skill 时：**

```
## Available Skills

### Available Skills

- **Skill Creator**: 用于创建新 skill 的辅助技能
- **Security Review** (aliases: sec, 安全): 安全审查技能

Use `/skill-<name>` to activate a skill.
```

**有活跃 skill 时（假设 skill-creator 活跃）：**

```
## Available Skills

### Active Skill: Skill Creator

# Skill Creator

你是一个专门用于创建新 skill 的辅助技能...
[完整 instructions 内容]

### Other Available Skills

- **Security Review** (aliases: sec, 安全): 安全审查技能

Use `/skill-<name>` to activate a skill.
```

### 4.4 Token 节省估算

假设有 5 个 skill，每个平均 500 token 的 instructions：
- 全量注入：5 × 500 = 2500 token
- 按需注入（无 active）：5 × 20（名称+描述） = 100 token
- 按需注入（1 个 active）：500 + 4 × 20 = 580 token

节省约 75-96%。

---

## 五、G8 — 模板变量替换增强

### 5.1 问题描述

`agent-nova.md` 中包含以下占位符（行 22-23）：

```markdown
3. 当前 Workflow 阶段: {{workflow_stage}}
4. 当前挂起交互: {{pending_interaction}}
```

这些占位符在 bootstrap.rs 中**无替换逻辑**，原样发送给 LLM。LLM 收到的是字面量 `{{workflow_stage}}`。

Phase 1 中已引入 `TemplateContext::render()` 的基础版本。Phase 2 增强如下：

### 5.2 设计方案

#### 5.2.1 增强 TemplateContext

```rust
// crates/nova-core/src/prompt.rs — 增强 TemplateContext

use once_cell::sync::Lazy;
use regex::Regex;

/// 模板变量正则匹配
static TEMPLATE_VAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap()
});

pub struct TemplateContext;

impl TemplateContext {
    /// 替换模板中的 {{key}} 占位符。
    ///
    /// - 已匹配的变量替换为对应值
    /// - 未匹配的占位符替换为空字符串（清理模式）
    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE.replace_all(template, |caps: &regex::Captures| {
            let key = &caps[1];
            vars.get(key).cloned().unwrap_or_default()
        }).to_string()
    }

    /// 替换模板中的 {{key}} 占位符（保留模式）。
    ///
    /// 已匹配的变量替换为对应值，未匹配的保持原样。
    /// 用于调试和渐进式替换场景。
    pub fn render_partial(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE.replace_all(template, |caps: &regex::Captures| {
            let key = &caps[1];
            match vars.get(key) {
                Some(value) => value.clone(),
                None => caps[0].to_string(), // 保持原样
            }
        }).to_string()
    }

    /// 提取模板中所有占位符的名称。
    pub fn extract_vars(template: &str) -> Vec<String> {
        TEMPLATE_VAR_RE.captures_iter(template)
            .map(|cap| cap[1].to_string())
            .collect()
    }
}
```

#### 5.2.2 预定义模板变量

定义标准模板变量集合：

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 预定义模板变量名称。
pub mod template_vars {
    /// 当前 workflow 阶段
    pub const WORKFLOW_STAGE: &str = "workflow_stage";
    /// 当前挂起交互
    pub const PENDING_INTERACTION: &str = "pending_interaction";
    /// 当前话题
    pub const TOPIC: &str = "topic";
    /// 约束条件
    pub const CONSTRAINTS: &str = "constraints";
    /// 候选方案列表
    pub const CANDIDATES: &str = "candidates";
    /// 已选方案
    pub const SELECTED_CANDIDATE: &str = "selected_candidate";
    /// 当前活跃 agent
    pub const ACTIVE_AGENT: &str = "active_agent";
}
```

#### 5.2.3 修改 bootstrap.rs 传入默认模板变量

```rust
// crates/nova-app/src/bootstrap.rs

let mut template_vars = HashMap::new();
template_vars.insert("workflow_stage".to_string(), "idle".to_string());
template_vars.insert("pending_interaction".to_string(), "none".to_string());
template_vars.insert("active_agent".to_string(), agent.display_name.clone());

let prompt_config = PromptConfig::new(
    agent.id.clone(),
    agent_prompt,
    config.workspace.clone(),
)
.with_environment(env_snapshot.clone())
.with_template_vars(template_vars);  // 新增 builder 方法
```

#### 5.2.4 PromptConfig 新增 with_template_vars

```rust
impl PromptConfig {
    pub fn with_template_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.template_vars = vars;
        self
    }
}
```

### 5.3 渲染结果示例

**替换前（agent-nova.md 原文）：**

```markdown
3. 当前 Workflow 阶段: {{workflow_stage}}
4. 当前挂起交互: {{pending_interaction}}
```

**替换后：**

```markdown
3. 当前 Workflow 阶段: idle
4. 当前挂起交互: none
```

### 5.4 依赖变更

需要确认 `once_cell` 和 `regex` 是否已在 workspace 依赖中：

```toml
# Cargo.toml [workspace.dependencies]
once_cell = "1"
regex = "1"
```

当前项目已使用 `regex`（在 skill.rs 的 frontmatter 解析等地方间接使用），`once_cell` 需要确认。如果不引入 `once_cell`，可以改用 `std::sync::LazyLock`（Rust 1.80+）或 `lazy_static`。

---

## 六、from_config() Phase 2 完整版本

整合 G5-G8 的所有变更后，`from_config()` 的完整实现：

```rust
impl SystemPromptBuilder {
    /// 从配置创建完整的 system prompt builder（Phase 2 版本）。
    ///
    /// 构建的 section 顺序：
    ///   Base (agent prompt) → BehaviorGuards → Skill → ProjectContext → Environment
    pub fn from_config(
        config: &PromptConfig,
        skills: &crate::skill::SkillRegistry,
    ) -> Self {
        let mut builder = Self::new();

        // L0: 平台身份（agent prompt 文件内容，经模板替换）
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
        if let Some(env) = &config.environment {
            builder = builder.environment_snapshot(env);
        }

        builder
    }
}
```

---

## 七、完整变更清单

| 文件 | 变更类型 | 变更说明 |
|------|----------|----------|
| `prompt.rs` | 新增 | `EnvironmentSnapshot` 结构体 |
| `prompt.rs` | 新增 | `EnvironmentSnapshot::collect()` 异步方法 |
| `prompt.rs` | 新增 | `EnvironmentSnapshot::to_prompt_text()` 方法 |
| `prompt.rs` | 新增 | `environment_snapshot()` builder 方法 |
| `prompt.rs` | 新增 | `load_project_context()` 函数 |
| `prompt.rs` | 新增 | `MAX_PROJECT_CONTEXT_CHARS` 常量 |
| `prompt.rs` | 修改 | `PromptConfig` 新增 `environment` 字段 |
| `prompt.rs` | 新增 | `PromptConfig::with_environment()` 方法 |
| `prompt.rs` | 新增 | `PromptConfig::with_template_vars()` 方法 |
| `prompt.rs` | 修改 | `TemplateContext::render()` 增强为正则替换 + 清理模式 |
| `prompt.rs` | 新增 | `TemplateContext::render_partial()` 保留模式 |
| `prompt.rs` | 新增 | `TemplateContext::extract_vars()` 变量提取 |
| `prompt.rs` | 新增 | `template_vars` 常量模块 |
| `prompt.rs` | 修改 | `from_config()` 整合 G5-G8 |
| `skill.rs` | 新增 | `generate_contextual_prompt()` 方法 |
| `skill.rs` | 修改 | `generate_system_prompt()` 添加 `#[deprecated]` |
| `config.rs` | 新增 | `ToolConfig.project_context_file` 字段 |
| `bootstrap.rs` | 修改 | 采集环境快照并传入 PromptConfig |
| `bootstrap.rs` | 修改 | 传入模板变量 |
| `Cargo.toml` | 可能修改 | 确认 chrono、once_cell 依赖 |

---

## 八、测试计划

### 8.1 单元测试

| 测试 | 文件 | 说明 |
|------|------|------|
| `env_snapshot_to_prompt_includes_cwd` | prompt.rs | 环境快照包含工作目录 |
| `env_snapshot_to_prompt_optional_git` | prompt.rs | git 信息为 None 时不输出对应行 |
| `env_snapshot_to_prompt_with_commits` | prompt.rs | 有最近提交时输出 |
| `load_project_context_finds_file` | prompt.rs | 存在 PROJECT.md 时加载 |
| `load_project_context_none_when_missing` | prompt.rs | 无文件时返回 None |
| `load_project_context_skips_empty` | prompt.rs | 空文件返回 None |
| `load_project_context_truncates_large` | prompt.rs | 超大文件被截断 |
| `contextual_prompt_no_active` | skill.rs | 无活跃 skill 时输出索引 |
| `contextual_prompt_with_active` | skill.rs | 活跃 skill 输出完整 instructions |
| `contextual_prompt_active_shows_others` | skill.rs | 活跃 skill 模式下其余 skill 仅名称 |
| `contextual_prompt_empty_registry` | skill.rs | 空 registry 返回空字符串 |
| `template_render_replaces_known` | prompt.rs | 已知变量正确替换 |
| `template_render_clears_unknown` | prompt.rs | 未知占位符替换为空 |
| `template_render_partial_keeps_unknown` | prompt.rs | partial 模式保持未知占位符 |
| `template_extract_vars` | prompt.rs | 正确提取所有变量名 |
| `from_config_full_integration` | prompt.rs | 完整 config 产生包含所有 section 的输出 |

### 8.2 集成测试

| 测试 | 说明 |
|------|------|
| bootstrap 产生的 prompt 包含 `## Environment` | 环境快照注入 |
| bootstrap 产生的 prompt 包含 `Working directory:` | 具体环境信息 |
| 带 PROJECT.md 的工作区有 `## Project Context` | 项目上下文注入 |
| 无 PROJECT.md 的工作区无 `## Project Context` | 跳过项目上下文 |
| agent-nova.md 中 `{{workflow_stage}}` 被替换为 `idle` | 模板替换 |
| skill 注入只包含名称索引（无活跃 skill） | 按需注入 |

### 8.3 运行验证

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```

---

## 九、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| `git` 命令在 CI 环境或容器中不可用 | 中 | `run_git` 返回 `Option`，失败静默跳过 |
| `chrono` 依赖引入 | 低 | 仅使用 `Local::now().format()`，可用 `time` crate 替代 |
| 项目上下文文件编码问题（非 UTF-8） | 低 | `read_to_string` 会报错，在 `match` 中静默跳过 |
| Skill 按需注入后 LLM 不知道如何激活 skill | 中 | 索引表中包含 `Use /skill-<name> to activate` 提示 |
| 模板变量 regex 性能 | 低 | 使用 `Lazy` 编译一次，后续复用 |
| `generate_system_prompt()` 被标记 deprecated 但仍有调用方 | 低 | Phase 2 确保所有调用方迁移到 `generate_contextual_prompt()` |
