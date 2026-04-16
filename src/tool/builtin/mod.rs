pub mod bash;
pub mod file_ops;
pub mod web_fetch;
pub mod web_search;

use crate::tool::ToolRegistry;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(registry: &mut ToolRegistry, config: &crate::config::AppConfig) {
    let _ = registry;

    registry.register(Box::new(bash::BashTool::new(&config.tool.bash)));

    {
        registry.register(Box::new(file_ops::ReadFileTool));
        registry.register(Box::new(file_ops::WriteFileTool));
    }

    registry.register(Box::new(web_search::WebSearchTool::new(&config.search)));

    registry.register(Box::new(web_fetch::WebFetchTool::new()));
}
