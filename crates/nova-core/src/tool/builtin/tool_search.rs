use crate::tool::{ToolDefinition, ToolOutput, ToolRegistry};
use anyhow::Result;
use serde_json::{json, Value};

pub const TOOL_NAME: &str = "ToolSearch";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_NAME.to_string(),
        description: "Search deferred tools and load their schemas on demand.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query or 'select:ToolName1,ToolName2' to load specific tools" },
                "max_results": { "type": "integer", "default": 5 }
            },
            "required": ["query"]
        }),
        defer_loading: false,
    }
}

pub async fn execute(registry: &ToolRegistry, input: Value) -> Result<ToolOutput> {
    let query = input["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'query'"))?;
    let max_results = input["max_results"].as_u64().unwrap_or(5) as usize;

    let content = if let Some(raw_selection) = query.strip_prefix("select:") {
        handle_selection(registry, raw_selection)
    } else {
        handle_search(registry, query, max_results)
    };

    Ok(ToolOutput {
        content,
        is_error: false,
    })
}

fn handle_selection(registry: &ToolRegistry, raw_selection: &str) -> String {
    let names: Vec<&str> = raw_selection
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();

    if names.is_empty() {
        return "No tool names were provided.".to_string();
    }

    let mut results = Vec::with_capacity(names.len());
    for name in names {
        if registry.resolve_deferred(name) {
            results.push(format!("Loaded tool: {}", name));
        } else if registry.has_loaded_tool(name) {
            results.push(format!("Tool '{}' is already loaded.", name));
        } else {
            results.push(format!("Tool '{}' not found.", name));
        }
    }
    results.join("\n")
}

fn handle_search(registry: &ToolRegistry, query: &str, max_results: usize) -> String {
    let query_lower = query.to_lowercase();
    let matches: Vec<String> = registry
        .deferred_definitions()
        .into_iter()
        .filter(|definition| {
            definition.name.to_lowercase().contains(&query_lower)
                || definition.description.to_lowercase().contains(&query_lower)
        })
        .take(max_results)
        .map(|definition| definition.name)
        .collect();

    if matches.is_empty() {
        "No matching deferred tools found.".to_string()
    } else {
        format!(
            "Found matching deferred tools: {}. Use 'select:Name' to load them.",
            matches.join(", ")
        )
    }
}
