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
}

#[derive(Debug, Clone)]
pub struct ParamMapping {
    pub name: String,
    pub param_type: String,
    pub arg: String,
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
            .filter(|p| !p.arg.is_empty() && !p.arg[0].is_empty())
            .map(|p| ParamMapping {
                name: p.name.clone(),
                param_type: p.param_type.clone(),
                arg: p.arg[0].clone(),
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
        assert_eq!(mappings[0].arg, "--url");
        assert_eq!(mappings[3].name, "days");
        assert_eq!(mappings[3].param_type, "integer");
        assert_eq!(mappings[3].arg, "--days");
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
        assert_eq!(mappings[0].arg, "-r");
        assert_eq!(mappings[1].arg, "-p");
    }
}
