pub mod bash;
pub mod file_ops;
pub mod subagent;
pub mod web_fetch;
pub mod web_search;

use crate::tool::ToolRegistry;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(registry: &mut ToolRegistry, config: &crate::config::AppConfig) {
    let _ = registry;

    registry.register(Box::new(bash::BashTool::new(&config.tool.bash)));

    {
        registry.register(Box::new(file_ops::ReadFileTool::new(None)));
        registry.register(Box::new(file_ops::WriteFileTool::new(None)));
    }

    registry.register(Box::new(subagent::SpawnSubagentTool::new(config.clone())));

    registry.register(Box::new(web_search::WebSearchTool::new(&config.search)));

    registry.register(Box::new(web_fetch::WebFetchTool::new()));
}
