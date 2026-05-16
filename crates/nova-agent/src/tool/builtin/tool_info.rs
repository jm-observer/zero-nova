use crate::tool::registry::{ToolMetadataView, ToolRegistry};
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

pub const TOOL_NAME: &str = "ToolInfo";

pub fn tool_definition() -> RegisteredToolDefinition {
    RegisteredToolDefinition {
        name: TOOL_NAME.to_string(),
        description:
            "Retrieve complete metadata for one or more tools, including full input schema and required parameters."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tool_names": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of tool names to query metadata for."
                },
                "include_schema": {
                    "type": "boolean",
                    "default": true,
                    "description": "Whether to include the full input schema in the response."
                }
            },
            "required": ["tool_names"]
        }),
        defer_loading: false,
    }
}

/// 工具元信息响应结构。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ToolInfoEntry {
    name: String,
    description: String,
    loaded: bool,
    deferred: bool,
    category: Option<String>,
    required_fields: Vec<String>,
    field_summaries: Vec<FieldValueSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_schema: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FieldValueSummary {
    name: String,
    r#type: String,
    description: Option<String>,
}

/// 由 `ToolRegistry::execute()` 调用的入口。
pub async fn execute(registry: &ToolRegistry, input: Value, context: Option<&ToolContext>) -> Result<ToolOutput> {
    let tool_names = parse_tool_names(&input)?;

    if tool_names.is_empty() {
        return Ok(ToolOutput {
            content: "Error: 'tool_names' array must contain at least one tool name.".to_string(),
            is_error: true,
        });
    }

    let include_schema = input["include_schema"].as_bool().unwrap_or(true);

    // 可见性过滤
    let visible_set: std::collections::HashSet<String> =
        context.map(|ctx| (*ctx.visible_tool_names).clone()).unwrap_or_default();

    let mut results = Vec::new();
    let mut not_found = Vec::new();
    let mut not_visible = Vec::new();

    for name in &tool_names {
        if !visible_set.is_empty() && !visible_set.contains(name) {
            not_visible.push(name.clone());
            continue;
        }

        match registry.tool_metadata(name).await {
            Some(meta) => {
                let entry = build_info_entry(&meta, include_schema);
                results.push(entry);
            }
            None => {
                not_found.push(name.clone());
            }
        }
    }

    let output = format_tool_info_output(&results, &not_found, &not_visible, include_schema);

    // 如果所有工具都找不到或不可见，标记为错误
    let is_error = results.is_empty() && (!not_found.is_empty() || !not_visible.is_empty());
    Ok(ToolOutput {
        content: output,
        is_error,
    })
}

fn parse_tool_names(input: &Value) -> Result<Vec<String>> {
    let tool_names = input
        .get("tool_names")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("'tool_names' must be a non-null array"))?;
    let mut parsed = Vec::with_capacity(tool_names.len());
    for value in tool_names {
        let name = value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("each item in 'tool_names' must be a string"))?;
        parsed.push(name.to_string());
    }
    Ok(parsed)
}

fn build_info_entry(meta: &ToolMetadataView, include_schema: bool) -> ToolInfoEntry {
    let required_fields = extract_required_fields(&meta.input_schema);
    let field_summaries = extract_field_summaries(&meta.input_schema);

    ToolInfoEntry {
        name: meta.name.clone(),
        description: meta.description.clone(),
        loaded: meta.loaded,
        deferred: meta.deferred,
        category: meta.category.as_ref().map(|c| c.to_string()),
        required_fields,
        field_summaries,
        input_schema: if include_schema {
            Some(meta.input_schema.clone())
        } else {
            None
        },
    }
}

fn extract_required_fields(schema: &Value) -> Vec<String> {
    match schema.get("required") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    }
}

fn extract_field_summaries(schema: &Value) -> Vec<FieldValueSummary> {
    match schema.get("properties") {
        Some(Value::Object(props)) => props
            .iter()
            .map(|(name, prop)| {
                let r#type = prop
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("object")
                    .to_string();
                let description = prop.get("description").and_then(|v| v.as_str()).map(String::from);
                FieldValueSummary {
                    name: name.clone(),
                    r#type,
                    description,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn format_tool_info_output(
    results: &[ToolInfoEntry],
    not_found: &[String],
    not_visible: &[String],
    include_schema: bool,
) -> String {
    let mut parts = Vec::new();

    if !results.is_empty() {
        parts.push(format!("Found {} tool(s):\n", results.len()));
        for entry in results {
            parts.push(format!("## {}\n", entry.name));
            parts.push(format!("{}\n", entry.description));
            let status_parts: Vec<&str> = if entry.loaded { vec!["loaded"] } else { vec!["deferred"] };
            parts.push(format!("Status: {}\n", status_parts.join(", ")));
            if let Some(ref cat) = entry.category {
                parts.push(format!("Category: {}\n", cat));
            }

            if !entry.required_fields.is_empty() {
                parts.push(format!("Required fields: {}\n", entry.required_fields.join(", ")));
            }

            if !entry.field_summaries.is_empty() {
                parts.push("Fields:\n".to_string());
                for field in &entry.field_summaries {
                    let desc = field.description.as_deref().unwrap_or("");
                    parts.push(format!(
                        "- `{}`: {}{}",
                        field.name,
                        field.r#type,
                        if desc.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", desc)
                        }
                    ));
                }
                parts.push("\n".to_string());
            }

            if include_schema {
                if let Some(ref schema) = entry.input_schema {
                    let schema_str = serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string());
                    parts.push("Full schema:\n```json\n".to_string());
                    parts.push(schema_str);
                    parts.push("\n```\n".to_string());
                }
            }

            parts.push("---\n\n".to_string());
        }
    }

    if !not_found.is_empty() {
        parts.push(format!("Not found: {}\n", not_found.join(", ")));
    }

    if !not_visible.is_empty() {
        parts.push(format!(
            "Not visible in current turn (query restricted): {}\n",
            not_visible.join(", ")
        ));
    }

    if results.is_empty() && not_found.is_empty() && not_visible.is_empty() {
        return "No tools matched the query.".to_string();
    }

    parts.join("")
}

pub struct ToolInfoTool {}

#[async_trait]
impl Tool for ToolInfoTool {
    fn definition(&self) -> RegisteredToolDefinition {
        tool_definition()
    }

    async fn execute(&self, _input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        // 这个 execute 路径不会被调用，因为 ToolRegistry::execute() 会直接分发到 builtin::tool_info::execute()
        Ok(ToolOutput {
            content: "ToolInfo: use tool_names parameter to query".to_string(),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::registry::ToolRegistry;
    use crate::tool::{Tool, ToolOutput};
    use std::collections::HashSet;
    use std::sync::Arc;

    struct TestTool {
        name: &'static str,
    }

    #[async_trait::async_trait]
    impl Tool for TestTool {
        fn definition(&self) -> RegisteredToolDefinition {
            RegisteredToolDefinition {
                name: self.name.to_string(),
                description: format!("{} description", self.name),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "file path" },
                        "count": { "type": "integer", "default": 10 }
                    },
                    "required": ["path"]
                }),
                defer_loading: false,
            }
        }

        async fn execute(&self, _input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
            Ok(ToolOutput {
                content: self.name.to_string(),
                is_error: false,
            })
        }
    }

    fn make_visible_set(names: &[&str]) -> Arc<HashSet<String>> {
        Arc::new(names.iter().map(|s| s.to_string()).collect())
    }

    #[tokio::test]
    async fn tool_info_returns_metadata_for_loaded_tool() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(TestTool { name: "Bash" })).await;

        let output = execute(
            &registry,
            json!({"tool_names": ["Bash"], "include_schema": true}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["Bash", "ToolInfo"]),
            }),
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("## Bash"));
        assert!(output.content.contains("Bash description"));
        assert!(output.content.contains("Full schema:"));
        assert!(output.content.contains("\"required\""));
        assert!(output.content.contains("\"path\""));
    }

    #[tokio::test]
    async fn tool_info_returns_summary_without_schema() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(TestTool { name: "Read" })).await;

        let output = execute(
            &registry,
            json!({"tool_names": ["Read"], "include_schema": false}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["Read", "ToolInfo"]),
            }),
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("## Read"));
        assert!(!output.content.contains("Full schema:"));
    }

    #[tokio::test]
    async fn tool_info_reports_not_found() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(TestTool { name: "Bash" })).await;

        let output = execute(
            &registry,
            json!({"tool_names": ["UnknownTool"]}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["Bash", "ToolInfo", "UnknownTool"]),
            }),
        )
        .await
        .unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("Not found: UnknownTool"));
    }

    #[tokio::test]
    async fn tool_info_rejects_invisible_tools() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(TestTool { name: "Bash" })).await;

        let output = execute(
            &registry,
            json!({"tool_names": ["Bash"]}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["ToolInfo"]),
            }),
        )
        .await
        .unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("Not visible in current turn"));
    }

    #[tokio::test]
    async fn tool_info_rejects_empty_tool_names() {
        let registry = ToolRegistry::new();

        let output = execute(
            &registry,
            json!({"tool_names": []}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["ToolInfo"]),
            }),
        )
        .await
        .unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("tool_names"));
    }

    #[tokio::test]
    async fn tool_info_queries_multiple_tools() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(TestTool { name: "Bash" })).await;
        registry.register(Box::new(TestTool { name: "Read" })).await;

        let output = execute(
            &registry,
            json!({"tool_names": ["Bash", "Read"]}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["Bash", "Read", "ToolInfo"]),
            }),
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("## Bash"));
        assert!(output.content.contains("## Read"));
        assert!(output.content.contains("Found 2 tool(s)"));
    }

    #[tokio::test]
    async fn tool_info_rejects_non_string_tool_name_items() {
        let registry = ToolRegistry::new();

        let error = execute(
            &registry,
            json!({"tool_names": ["Bash", 1]}),
            Some(&ToolContext {
                event_tx: tokio::sync::mpsc::channel(1).0,
                tool_use_id: "test".to_string(),
                session_id: "test".to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: make_visible_set(&["ToolInfo"]),
            }),
        )
        .await;

        assert!(error.is_err());
        assert!(error.err().unwrap().to_string().contains("must be a string"));
    }
}
