use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};
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
    } else if let Some(raw_selection) = query.strip_prefix("load:") {
        handle_category_selection(registry, raw_selection)
    } else {
        handle_search(registry, query, max_results)
    };

    Ok(ToolOutput {
        content,
        is_error: false,
    })
}

/// 使用 `select:category:CategoryName` 语法加载指定类别的工具。
fn handle_category_selection(registry: &ToolRegistry, category: &str) -> String {
    use crate::tool::DeferredToolCategory;

    let category = match category.to_lowercase().as_str() {
        "task" => DeferredToolCategory::Task,
        "skill" => DeferredToolCategory::Skill,
        "search" => DeferredToolCategory::Search,
        "system" => DeferredToolCategory::System,
        _ => {
            return format!("Unknown category: {}", category);
        }
    };

    registry.load_deferred_by_category(&category, true);
    format!("Loaded all tools for category: {}", category)
}

fn handle_selection(registry: &ToolRegistry, raw_selection: &str) -> String {
    // Check if it's select:category:CategoryName
    if let Some(category_part) = raw_selection.strip_prefix("category:") {
        return handle_category_selection(registry, category_part);
    }

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

/// ToolSearch 工具结构体（用于 deferred 工厂）。
pub struct ToolSearchTool {}

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn definition(&self) -> ToolDefinition {
        tool_definition()
    }

    async fn execute(&self, _input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        // ToolSearch 本身不通过 ToolRegistry.execute() 调用，
        // 而是由 builtin::tool_search::execute() 处理。
        // 这里实现一个基本版本以保持 Tool trait 兼容性。
        Ok(ToolOutput {
            content: "ToolSearch: use select: or search query".to_string(),
            is_error: false,
        })
    }
}
