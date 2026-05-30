use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Debug, Deserialize)]
pub struct ToolFile {
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Deserialize)]
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
    /// 工具执行超时（秒）。未声明时回退到 executor 的 `DEFAULT_TIMEOUT`（30s）。
    /// 适合 douyin_list_works 这类需要拉多页 / 长时间外部 API 调用的工具。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

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
    /// 工具执行超时（秒）。`None` → executor 用 `DEFAULT_TIMEOUT`（30s）。
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ParamMapping {
    pub name: String,
    pub param_type: String,
    /// `Some(flag)` 表示命名参数（如 `--id`）；`None` 表示位置参数（按 `ParamMapping`
    /// 在 `param_mappings` 中的相对顺序透传 value，不带 flag 前缀）。
    pub arg: Option<String>,
    pub required: bool,
}

impl ToolSpec {
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
                timeout_secs: self.timeout_secs,
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
            .map(|p| {
                let arg = p.arg.iter().find(|s| !s.is_empty()).cloned();
                ParamMapping {
                    name: p.name.clone(),
                    param_type: p.param_type.clone(),
                    arg,
                    required: p.required,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB_TOOL_TOML: &str = r#"
[[tools]]
name = "github-commit-info"
description = "获取GitHub仓库指定时间范围内的commit信息"
type = "command"
command = "github-commit-info"
cwd = false

[[tools.parameters]]
name = "url"
description = "GitHub仓库URL (如: https://github.com/golang/go)"
type = "string"
required = true
arg = ["--url"]

[[tools.parameters]]
name = "branch"
description = "分支名称 (如不指定则自动获取默认分支)"
type = "string"
required = false
arg = ["--branch"]

[[tools.parameters]]
name = "start_date"
description = "起始日期, 格式: yyyy-MM-dd (默认昨天)"
type = "string"
required = false
arg = ["--start-date"]

[[tools.parameters]]
name = "days"
description = "从起始日期开始的天数 (默认1)"
type = "integer"
required = false
arg = ["--days"]

[[tools.parameters]]
name = "output"
description = "输出文件路径 (默认为stdout)"
type = "string"
required = false
arg = ["--output"]
"#;

    #[test]
    fn test_parse_tool_file() {
        let tool_file: ToolFile = toml::from_str(GITHUB_TOOL_TOML).unwrap();
        assert_eq!(tool_file.tools.len(), 1);

        let spec = &tool_file.tools[0];
        assert_eq!(spec.name, "github-commit-info");
        assert_eq!(spec.tool_type, "command");
        assert_eq!(spec.command, "github-commit-info");
        assert!(!spec.cwd);
        assert_eq!(spec.parameters.len(), 5);
        assert!(spec.parameters[0].required);
        assert!(!spec.parameters[1].required);
        assert_eq!(spec.parameters[3].param_type, "integer");
    }

    #[test]
    fn test_build_json_schema() {
        let tool_file: ToolFile = toml::from_str(GITHUB_TOOL_TOML).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        let def = spec.into_definition();

        let schema = &def.input_schema;
        assert_eq!(schema["type"], "object");

        let props = schema["properties"].as_object().unwrap();
        assert_eq!(props.len(), 5);
        assert_eq!(props["url"]["type"], "string");
        assert_eq!(props["days"]["type"], "integer");

        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "url");
    }

    #[test]
    fn test_param_mapping() {
        let tool_file: ToolFile = toml::from_str(GITHUB_TOOL_TOML).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        let def = spec.into_definition();

        let mappings = &def.execution.param_mappings;
        assert_eq!(mappings.len(), 5);
        assert_eq!(mappings[0].name, "url");
        assert_eq!(mappings[0].arg.as_deref(), Some("--url"));
        assert_eq!(mappings[3].name, "days");
        assert_eq!(mappings[3].param_type, "integer");
        assert_eq!(mappings[3].arg.as_deref(), Some("--days"));
    }

    #[test]
    fn test_parse_tool_with_multiple_args() {
        let toml_str = r#"
[[tools]]
name = "cargo build"
description = "Build a cargo project"
type = "command"
command = "cargo"
subcommands = ["build"]
cwd = true

[[tools.parameters]]
name = "release"
description = "Build in release mode"
type = "boolean"
required = false
arg = ["-r", "--release"]

[[tools.parameters]]
name = "package"
description = "Package to build"
type = "string"
required = false
arg = ["-p", "--package"]
"#;
        let tool_file: ToolFile = toml::from_str(toml_str).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        let def = spec.into_definition();

        assert_eq!(def.execution.command, "cargo");
        assert_eq!(def.execution.subcommands, vec!["build"]);
        assert!(def.execution.cwd);

        let mappings = &def.execution.param_mappings;
        assert_eq!(mappings[0].arg.as_deref(), Some("-r"));
        assert_eq!(mappings[1].arg.as_deref(), Some("-p"));
    }

    #[test]
    fn test_positional_param_mapping() {
        // alarm-cli cancel <ID> 这种位置参数：toml 里 `id` 未声明 `arg`
        let toml_str = r#"
[[tools]]
name = "alarm-cli-cancel"
description = "Cancel an alarm by its UUID."
type = "command"
command = "alarm-cli"
subcommands = ["cancel"]
cwd = false

[[tools.parameters]]
name = "id"
description = "Alarm UUID"
type = "string"
required = true

[[tools.parameters]]
name = "workspace"
description = "Workspace directory"
type = "string"
required = false
arg = ["--workspace", "-w"]
"#;
        let tool_file: ToolFile = toml::from_str(toml_str).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        let def = spec.into_definition();

        let mappings = &def.execution.param_mappings;
        // 位置参数也必须出现在 mappings 中（旧实现会被 filter 掉，导致执行时静默丢弃）
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings[0].name, "id");
        assert!(mappings[0].arg.is_none(), "id 必须是位置参数（arg=None）");
        assert!(mappings[0].required);
        assert_eq!(mappings[1].name, "workspace");
        assert_eq!(mappings[1].arg.as_deref(), Some("--workspace"));
        assert!(!mappings[1].required);
    }

    /// 未声明 timeout_secs 时回退 None，执行器侧由 `DEFAULT_TIMEOUT` 兜底。
    #[test]
    fn test_parse_timeout_secs_optional_and_default_none() {
        let toml_str = r#"
[[tools]]
name = "fast"
description = "no explicit timeout"
type = "command"
command = "echo"
cwd = false
"#;
        let tool_file: ToolFile = toml::from_str(toml_str).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        assert!(
            spec.timeout_secs.is_none(),
            "未声明 timeout_secs 时 ToolSpec.timeout_secs 应为 None"
        );
        let def = spec.into_definition();
        assert!(def.execution.timeout_secs.is_none(), "into_definition 应原样透传 None");
    }

    /// 显式声明 `timeout_secs = 180`，运行时按 180s 计算。
    #[test]
    fn test_parse_timeout_secs_explicit_value() {
        let toml_str = r#"
[[tools]]
name = "long"
description = "needs 180s, e.g. douyin_list_works pulling 60 pages"
type = "command"
command = "douyin"
subcommands = ["list-works"]
cwd = false
timeout_secs = 180
"#;
        let tool_file: ToolFile = toml::from_str(toml_str).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        assert_eq!(spec.timeout_secs, Some(180));
        let def = spec.into_definition();
        assert_eq!(def.execution.timeout_secs, Some(180));
    }

    /// `timeout_secs = 0` 合法解析为 Some(0)；语义上"立即超时"，由用户自行承担。
    #[test]
    fn test_parse_timeout_secs_zero() {
        let toml_str = r#"
[[tools]]
name = "zero-timeout"
description = "edge case"
type = "command"
command = "true"
cwd = false
timeout_secs = 0
"#;
        let tool_file: ToolFile = toml::from_str(toml_str).unwrap();
        let spec = tool_file.tools.into_iter().next().unwrap();
        assert_eq!(spec.timeout_secs, Some(0));
    }

    /// `timeout_secs` 写成字符串 → toml 解析失败（u64 类型校验）。防呆。
    #[test]
    fn test_parse_timeout_secs_string_rejected() {
        let toml_str = r#"
[[tools]]
name = "bad"
description = "wrong type"
type = "command"
command = "true"
cwd = false
timeout_secs = "180"
"#;
        let res: Result<ToolFile, _> = toml::from_str(toml_str);
        assert!(res.is_err(), "字符串型 timeout_secs 应被拒绝");
    }
}
