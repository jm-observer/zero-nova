pub mod bash;
pub mod file_ops;
pub mod web_fetch;
pub mod web_search;

use crate::tool::ToolRegistry;

/// 注册所有内置工具到 registry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    let _ = registry;
    #[cfg(feature = "tool-bash")]
    registry.register(Box::new(bash::BashTool));

    #[cfg(feature = "tool-file-ops")]
    {
        registry.register(Box::new(file_ops::ReadFileTool));
        registry.register(Box::new(file_ops::WriteFileTool));
    }

    #[cfg(feature = "tool-web-search")]
    if let Ok(tool) = web_search::WebSearchTool::from_env() {
        registry.register(Box::new(tool));
    }

    #[cfg(feature = "tool-web-fetch")]
    registry.register(Box::new(web_fetch::WebFetchTool::new()));
}
