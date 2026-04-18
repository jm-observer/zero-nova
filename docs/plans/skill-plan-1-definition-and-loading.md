# Plan 1：Skill 定义与加载

> 前置依赖：无 | 预期产出：`src/skill/definition.rs`, `src/skill/registry.rs`, `src/skill/mod.rs`, 配置扩展

## 1. 目标

实现 Skill 的文件定义格式解析和**递归目录扫描**，使系统能在启动时从任意深度的目录树中加载所有叶子 skill。

## 2. 文件格式规范

### 2.1 目录结构

支持任意深度嵌套。节点类型由是否存在 `skill.toml` 决定：

```
skills/
├── coding/                          ← Group（无 skill.toml，仅分组）
│   ├── commit/                      ← Skill  slug = "coding/commit"
│   │   ├── skill.toml
│   │   └── prompt.md
│   ├── review/                      ← Skill  slug = "coding/review"
│   │   ├── skill.toml
│   │   └── prompt.md
│   └── debug/                       ← Group
│       ├── frontend/                ← Skill  slug = "coding/debug/frontend"
│       │   ├── skill.toml
│       │   └── prompt.md
│       └── backend/                 ← Skill  slug = "coding/debug/backend"
│           ├── skill.toml
│           └── prompt.md
├── writing/                         ← Group
│   ├── blog/                        ← Skill  slug = "writing/blog"
│   │   ├── skill.toml
│   │   └── prompt.md
│   └── docs/                        ← Skill  slug = "writing/docs"
│       ├── skill.toml
│       └── prompt.md
└── tech-consult/                    ← Skill  slug = "tech-consult"（根级别叶子）
    ├── skill.toml
    └── prompt.md
```

**节点判定规则：**

| 条件 | 类型 | 行为 |
|------|------|------|
| 目录含 `skill.toml` + `prompt.md` | **Skill（叶子）** | 注册为可激活 skill |
| 目录含子目录但无 `skill.toml` | **Group（分组）** | 递归扫描子目录，自身不注册 |
| 目录含 `skill.toml` 但无 `prompt.md` | **无效** | 跳过并记录 warn |
| 目录含 `skill.toml` 且含子目录 | **Skill（叶子）** | 注册为 skill，忽略子目录 |

**slug 规则：** skill 目录相对于 `skills/` 根目录的路径，使用 `/` 分隔（跨平台统一）。

### 2.2 skill.toml 完整格式

```toml
[skill]
name = "前端调试助手"                                        # 显示名称
description = "调试前端问题，包括 CSS 布局、JS 报错、React 组件渲染等"  # 场景描述（LLM 分类器依据）
version = "1.0"                                              # 版本号（信息性）

[tools]
allowed = ["bash", "read_file"]      # 工具白名单，空数组 [] = 不限制

[config]
max_iterations = 10                   # 可选：覆盖 agent 默认迭代上限
```

> **设计决策：** 没有 `[trigger]` 配置。Skill 路由完全通过 LLM 意图分类实现（见 Plan 2），分类器根据 `description` 字段判断用户消息应匹配哪个 skill。因此 `description` 应当准确描述该 skill 的适用场景和能力范围。

**字段规则：**

| 字段 | 必需 | 默认值 | 说明 |
|------|------|--------|------|
| `skill.name` | 是 | - | 显示名称 |
| `skill.description` | 是 | - | 场景描述，LLM 分类器依据此字段做意图匹配 |
| `skill.version` | 否 | `"1.0"` | 信息性字段 |
| `tools.allowed` | 否 | `[]` | 空 = 全部工具可用 |
| `config.max_iterations` | 否 | 继承全局配置 | skill 级别覆盖 |

### 2.3 prompt.md 格式

纯 Markdown 文本，无特殊语法要求。内容会原样注入到 system prompt 中。

## 3. 数据结构设计

### 3.1 `src/skill/definition.rs`

```rust
use serde::Deserialize;
use anyhow::Result;
use std::path::Path;

/// TOML 文件反序列化结构（与文件格式一一对应）
#[derive(Debug, Deserialize)]
struct SkillToml {
    skill: SkillMeta,
    #[serde(default)]
    tools: ToolsConfig,
    #[serde(default)]
    config: SkillOverrideConfig,
}

#[derive(Debug, Deserialize)]
struct SkillMeta {
    name: String,
    description: String,
    #[serde(default = "default_version")]
    version: String,
}

fn default_version() -> String {
    "1.0".to_string()
}

#[derive(Debug, Default, Deserialize)]
struct ToolsConfig {
    #[serde(default)]
    allowed: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SkillOverrideConfig {
    max_iterations: Option<usize>,
}

/// 运行时使用的 Skill 定义（解析后的最终形态，叶子节点）
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    /// 唯一标识：相对于 skills/ 根目录的路径，如 "coding/debug/frontend"
    pub slug: String,
    /// 显示名称
    pub name: String,
    /// 场景描述（LLM 分类器依据此字段做意图匹配）
    pub description: String,
    /// 版本
    pub version: String,
    /// 行为提示词（prompt.md 内容）
    pub prompt: String,
    /// 工具约束
    pub tool_constraint: SkillToolConstraint,
    /// 覆盖配置
    pub config_override: SkillConfigOverride,
}

#[derive(Debug, Clone)]
pub struct SkillToolConstraint {
    /// 空 Vec 表示不限制
    pub allowed: Vec<String>,
}

impl SkillToolConstraint {
    pub fn allows_all(&self) -> bool {
        self.allowed.is_empty()
    }

    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.allows_all() || self.allowed.iter().any(|t| t == tool_name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillConfigOverride {
    pub max_iterations: Option<usize>,
}
```

### 3.2 解析函数

```rust
impl SkillDefinition {
    /// 从目录加载单个 skill，slug 由调用方传入
    pub fn load_from_dir(dir: &Path, slug: String) -> Result<Self> {
        // 读取 skill.toml
        let toml_path = dir.join("skill.toml");
        let toml_content = std::fs::read_to_string(&toml_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", toml_path, e))?;
        let toml: SkillToml = toml::from_str(&toml_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {:?}: {}", toml_path, e))?;

        // 读取 prompt.md
        let prompt_path = dir.join("prompt.md");
        let prompt = std::fs::read_to_string(&prompt_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", prompt_path, e))?;

        Ok(Self {
            slug,
            name: toml.skill.name,
            description: toml.skill.description,
            version: toml.skill.version,
            prompt,
            tool_constraint: SkillToolConstraint {
                allowed: toml.tools.allowed,
            },
            config_override: SkillConfigOverride {
                max_iterations: toml.config.max_iterations,
            },
        })
    }
}
```

**改动点对比旧版：** `slug` 不再从 `dir.file_name()` 取单层目录名，而是由 `SkillRegistry` 递归扫描时计算完整相对路径后传入。

### 3.3 `src/skill/registry.rs`

```rust
use super::definition::SkillDefinition;
use anyhow::Result;
use std::path::Path;

/// Skill 注册表，管理所有已加载的叶子 skill（扁平列表）
pub struct SkillRegistry {
    skills: Vec<SkillDefinition>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    /// 从目录递归加载所有 skill
    ///
    /// 递归遍历目录树，将所有叶子节点（含 skill.toml 的目录）注册为 skill。
    /// slug 为相对于 root_dir 的路径（使用 `/` 分隔）。
    /// 解析失败的 skill 会记录警告日志但不阻断加载。
    pub fn load_from_directory(root_dir: &Path) -> Result<Self> {
        let mut registry = Self::new();

        if !root_dir.exists() {
            log::warn!("Skill directory does not exist: {:?}", root_dir);
            return Ok(registry);
        }

        registry.scan_recursive(root_dir, root_dir)?;

        log::info!("Loaded {} skills total", registry.skills.len());
        Ok(registry)
    }

    /// 递归扫描目录
    fn scan_recursive(&mut self, current_dir: &Path, root_dir: &Path) -> Result<()> {
        let entries = std::fs::read_dir(current_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let has_skill_toml = path.join("skill.toml").exists();
            let has_prompt_md = path.join("prompt.md").exists();

            if has_skill_toml && has_prompt_md {
                // 叶子节点：加载为 skill
                let slug = self.compute_slug(&path, root_dir);
                match SkillDefinition::load_from_dir(&path, slug.clone()) {
                    Ok(skill) => {
                        log::info!(
                            "Loaded skill: {} (slug: {}, description: {})",
                            skill.name,
                            skill.slug,
                            skill.description
                        );
                        self.skills.push(skill);
                    }
                    Err(e) => {
                        log::warn!("Failed to load skill from {:?}: {}", path, e);
                    }
                }
                // 叶子节点不再递归子目录
            } else if has_skill_toml && !has_prompt_md {
                // 有 skill.toml 但缺少 prompt.md → 无效
                log::warn!(
                    "Skill directory {:?} has skill.toml but missing prompt.md, skipping",
                    path
                );
            } else {
                // 无 skill.toml → Group 节点，递归进入
                self.scan_recursive(&path, root_dir)?;
            }
        }
        Ok(())
    }

    /// 计算 slug：相对于 root_dir 的路径，使用 `/` 统一分隔
    fn compute_slug(&self, skill_dir: &Path, root_dir: &Path) -> String {
        let relative = skill_dir
            .strip_prefix(root_dir)
            .expect("skill_dir must be under root_dir");

        // 统一使用 `/` 分隔（跨平台）
        relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// 获取所有已加载的 skill（扁平列表）
    pub fn skills(&self) -> &[SkillDefinition] {
        &self.skills
    }

    /// 按 slug 查找 skill（如 "coding/debug/frontend"）
    pub fn find_by_slug(&self, slug: &str) -> Option<&SkillDefinition> {
        self.skills.iter().find(|s| s.slug == slug)
    }

    /// 按 name 查找 skill
    pub fn find_by_name(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// 获取 skill 总数
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3.4 `src/skill/mod.rs`

```rust
pub mod definition;
pub mod registry;

pub use definition::{SkillDefinition, SkillToolConstraint, SkillConfigOverride};
pub use registry::SkillRegistry;
```

## 4. 配置扩展

### 4.1 `src/config.rs` 新增

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillConfig {
    /// skill 定义文件目录（支持任意深度嵌套）
    #[serde(default = "default_skill_directory")]
    pub directory: String,
    /// 是否启用 skill 系统
    #[serde(default = "default_skill_enabled")]
    pub enabled: bool,
    /// 分类器模型配置
    #[serde(default)]
    pub classifier: ClassifierConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClassifierConfig {
    /// 分类器使用的模型（小模型，快速低成本）
    #[serde(default = "default_classifier_model")]
    pub model: String,
    /// 分类器最大输出 token
    #[serde(default = "default_classifier_max_tokens")]
    pub max_tokens: u32,
}

fn default_skill_directory() -> String {
    "./skills".to_string()
}

fn default_skill_enabled() -> bool {
    true
}

fn default_classifier_model() -> String {
    "claude-3-5-haiku-latest".to_string()
}

fn default_classifier_max_tokens() -> u32 {
    128
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            model: default_classifier_model(),
            max_tokens: default_classifier_max_tokens(),
        }
    }
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            directory: default_skill_directory(),
            enabled: default_skill_enabled(),
            classifier: ClassifierConfig::default(),
        }
    }
}

// AppConfig 新增字段
pub struct AppConfig {
    // ... 现有字段 ...
    #[serde(default)]
    pub skill: SkillConfig,
}
```

**config.toml 示例：**

```toml
[skill]
directory = "./skills"
enabled = true

[skill.classifier]
model = "claude-3-5-haiku-latest"
max_tokens = 128
```

## 5. 集成点

### 5.1 启动时加载

在 `src/gateway/mod.rs` 的 `start_server()` 中，创建 `ToolRegistry` 之后加载 skill：

```rust
// 加载 skill（递归扫描目录树）
let skill_registry = if config.skill.enabled {
    let skill_dir = std::path::Path::new(&config.skill.directory);
    SkillRegistry::load_from_directory(skill_dir)?
} else {
    SkillRegistry::new()
};
```

### 5.2 存储位置

`SkillRegistry` 需要作为 `AppState` 的一部分共享给所有请求：

```rust
pub struct AppState<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub sessions: SessionStore,
    pub skills: SkillRegistry,       // 新增
}
```

## 6. 实施步骤

| 步骤 | 操作 | 涉及文件 |
|------|------|----------|
| 1 | 创建 `src/skill/` 目录和模块文件 | `src/skill/mod.rs`, `definition.rs`, `registry.rs` |
| 2 | 实现 `SkillDefinition` 和 TOML 解析（slug 由外部传入） | `src/skill/definition.rs` |
| 3 | 实现 `SkillRegistry`，含递归扫描和 slug 计算 | `src/skill/registry.rs` |
| 4 | 在 `config.rs` 中新增 `SkillConfig` + `ClassifierConfig` | `src/config.rs` |
| 5 | 在 `lib.rs` 中注册 `skill` 模块 | `src/lib.rs` |
| 6 | 在 `gateway/mod.rs` 中集成加载逻辑 | `src/gateway/mod.rs` |
| 7 | 在 `AppState` 中添加 `skills` 字段 | `src/gateway/router.rs` |
| 8 | 创建多层级示例 skill 目录用于测试 | `skills/coding/commit/`, `skills/coding/debug/frontend/`, `skills/tech-consult/` |

## 7. 验证标准

- [ ] `cargo build` 通过
- [ ] 启动时能正确递归加载 `skills/` 目录下所有层级的 skill
- [ ] 根级别 skill（如 `skills/tech-consult/`）slug 为 `tech-consult`
- [ ] 嵌套 skill（如 `skills/coding/debug/frontend/`）slug 为 `coding/debug/frontend`
- [ ] Group 目录（无 `skill.toml`）被正确跳过，递归进入子目录
- [ ] 有 `skill.toml` 但缺 `prompt.md` 的目录跳过并记录警告
- [ ] 叶子节点不再递归其子目录
- [ ] `skills/` 目录不存在时不报错，正常启动
- [ ] `skill.enabled = false` 时不加载任何 skill
- [ ] slug 在 Windows 上也使用 `/` 分隔，不出现 `\`
