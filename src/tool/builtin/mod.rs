pub mod bash;
pub mod file_ops;
pub mod web_fetch;
pub mod web_search;

use crate::tool::ToolRegistry;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    let _ = registry;

    registry.register(Box::new(bash::BashTool::new()));

    {
        registry.register(Box::new(file_ops::ReadFileTool));
        registry.register(Box::new(file_ops::WriteFileTool));
    }

    match web_search::WebSearchTool::from_env() {
        Ok(tool) => registry.register(Box::new(tool)),
        Err(e) => log::error!("Failed to register web_search tool: {}", e),
    }

    registry.register(Box::new(web_fetch::WebFetchTool::new()));
}
