# Plan 1: Tool 定义文件解析

## 前置依赖

无

## 任务目标

实现 `.toml` 格式的外部 tool 定义文件解析，将 mcp-tool-generator 生成的参数格式转换为 JSON Schema + 命令行参数映射。

## 执行范围

- **必须修改**：`crates/nova-agent-config/src/models.rs`（新增 `ToolConfig.tools_dir` 字段）
- **必须新增**：`crates/nova-agent/src/tool/external/mod.rs`、`crates/nova-agent/src/tool/external/schema.rs`
- **允许修改**：`crates/nova-agent/src/tool/mod.rs`（导出新模块）
- **禁止修改**：`registry.rs`、任何 builtin tool

## Agent 执行步骤

### 步骤 1：在 `ToolConfig` 新增 `tools_dir` 字段

修改 `crates/nova-agent-config/src/models.rs`：

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    pub skills_dir: Option<String>,
    pub prompts_dir: Option<String>,
    pub project_context_file: Option<String>,
    #[serde(default)]
    pub default_policy: Option<String>,
    /// 外部 tool 定义文件目录。为空时不加载外部 tool。
    #[serde(default)]
    pub tools_dir: Option<String>,
}
```

### 步骤 2：定义解析数据结构

新增 `crates/nova-agent/src/tool/external/schema.rs`：

```rust
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// 对应 .toml 文件的顶层结构
#[derive(Debug, Deserialize)]
pub struct ToolFile {
    pub tools: Vec<ToolSpec>,
}

/// 单个 tool 的声明
#[derive(Debug, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub command: String,
    #[serde(default)]
    pub subcommands: Vec<String>,
    #[serde(default)]
    pub cwd: bool,
    #[serde(default)]
    pub parameters: Vec<ParamSpec>,
}

/// 单个参数的声明
#[derive(Debug, Clone, Deserialize)]
pub struct ParamSpec {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub arg: Vec<String>,
}

/// 转换后的内部表示：JSON Schema + 命令行映射
#[derive(Debug, Clone)]
pub struct ExternalToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub execution: CommandExecution,
}

#[derive(Debug, Clone)]
pub struct CommandExecution {
    pub command: String,
    pub subcommands: Vec<String>,
    pub cwd: bool,
    pub param_mappings: Vec<ParamMapping>,
}

#[derive(Debug, Clone)]
pub struct ParamMapping {
    pub name: String,
    pub param_type: String,
    pub arg: String,
}

impl ToolSpec {
    /// 将 ToolSpec 转换为 ExternalToolDefinition（JSON Schema + 命令映射）
    pub fn into_definition(self) -> ExternalToolDefinition {
        let input_schema = self.build_json_schema();
        let param_mappings = self.build_param_mappings();

        ExternalToolDefinition {
            name: self.name,
            description: self.description,
            input_schema,
            execution: CommandExecution {
                command: self.command,
                subcommands: self.subcommands,
                cwd: self.cwd,
                param_mappings,
            },
        }
    }

    fn build_json_schema(&self) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let schema_type = match param.param_type.as_str() {
                "boolean" => "boolean",
                "integer" => "integer",
                "number" => "number",
                _ => "string",
            };
            properties.insert(
                param.name.clone(),
                json!({
                    "type": schema_type,
                    "description": param.description
                }),
            );
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    fn build_param_mappings(&self) -> Vec<ParamMapping> {
        self.parameters
            .iter()
            .filter(|p| !p.arg.is_empty())
            .map(|p| ParamMapping {
                name: p.name.clone(),
                param_type: p.param_type.clone(),
                arg: p.arg[0].clone(),
            })
            .collect()
    }
}
```

### 步骤 3：实现文件加载函数

新增 `crates/nova-agent/src/tool/external/mod.rs`：

```rust
pub mod schema;

use anyhow::{Context, Result};
use schema::{ExternalToolDefinition, ToolFile};
use std::path::Path;

/// 从单个 .toml 文件解析 tool 定义
pub fn load_tool_file(path: &Path) -> Result<Vec<ExternalToolDefinition>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read tool file: {}", path.display()))?;
    let tool_file: ToolFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse tool file: {}", path.display()))?;
    Ok(tool_file.tools.into_iter().map(|spec| spec.into_definition()).collect())
}

/// 扫描目录，加载所有 tool 定义
pub fn load_tools_from_dir(dir: &Path) -> Result<Vec<ExternalToolDefinition>> {
    let mut definitions = Vec::new();
    if !dir.exists() {
        return Ok(definitions);
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read tools directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // 目录模式：tools.d/<name>/<name>.toml
            for sub_entry in std::fs::read_dir(&path)? {
                let sub_path = sub_entry?.path();
                if sub_path.extension().is_some_and(|ext| ext == "toml") {
                    match load_tool_file(&sub_path) {
                        Ok(defs) => definitions.extend(defs),
                        Err(e) => log::warn!("skipping tool file {}: {}", sub_path.display(), e),
                    }
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            // 扁平模式：tools.d/<name>.toml
            match load_tool_file(&path) {
                Ok(defs) => definitions.extend(defs),
                Err(e) => log::warn!("skipping tool file {}: {}", path.display(), e),
            }
        }
    }
    Ok(definitions)
}
```

## 目标数据结构

| 结构 | 用途 |
|------|------|
| `ToolFile` | .toml 反序列化入口 |
| `ToolSpec` | 单个 tool 原始声明 |
| `ParamSpec` | 参数原始声明（含 arg 映射） |
| `ExternalToolDefinition` | 转换后的内部表示 |
| `CommandExecution` | 命令执行信息 |
| `ParamMapping` | 参数名 → CLI flag 映射 |

## 行为规则

| 输入 | 处理 | 输出 |
|------|------|------|
| 合法 .toml 文件 | 解析 + 转换 | `Vec<ExternalToolDefinition>` |
| 不存在的目录 | 返回空列表 | `Ok(vec![])` |
| 无法解析的 .toml | warn 日志，跳过 | 不影响其他文件 |
| 参数 type="boolean" | schema type="boolean" | 执行时 true → 传 arg，false → 不传 |
| 参数 type="string"/"integer" | schema 对应类型 | 执行时传 `arg value` |
| 参数无 arg 字段 | 不生成 ParamMapping | 作为位置参数处理（后续扩展） |

## 禁止事项

- 禁止修改 `registry.rs`
- 禁止修改任何 builtin tool
- 禁止引入新的外部依赖（`toml` 和 `serde_json` 已在 workspace 中）

## 测试要求

- 测试文件：`crates/nova-agent/src/tool/external/schema.rs` 内 `#[cfg(test)] mod tests`
- 测试名：`test_parse_tool_file`、`test_build_json_schema`、`test_param_mapping`
- 输入：使用 `tmp/tools.d/github-commit-info/github-commit-info.toml` 的内容作为测试用例
- 验证命令：`cargo test -p nova-agent test_parse_tool_file test_build_json_schema test_param_mapping`

## 完成条件

- [ ] `ToolFile`/`ToolSpec`/`ParamSpec` 可正确反序列化 mcp-tool-generator 格式
- [ ] `into_definition()` 生成正确的 JSON Schema
- [ ] `ParamMapping` 保留第一个 arg 作为 CLI flag
- [ ] `load_tools_from_dir` 支持目录和扁平两种布局
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
