pub mod bash;
pub mod file_ops;
pub mod web_fetch;
pub mod web_search;

use crate::tool::ToolRegistry;

/// 注册所有内置工具到 registry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    let _ = registry;

    registry.register(Box::new(bash::BashTool));

    {
        registry.register(Box::new(file_ops::ReadFileTool));
        registry.register(Box::new(file_ops::WriteFileTool));
    }

    if let Ok(tool) = web_search::WebSearchTool::from_env() {
        registry.register(Box::new(tool));
    }

    registry.register(Box::new(web_fetch::WebFetchTool::new()));
}
