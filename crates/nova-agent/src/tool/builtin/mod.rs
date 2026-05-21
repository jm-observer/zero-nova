pub mod agent;
pub mod bash;
pub mod edit;
pub mod orchestrate_hook;
pub mod orchestrate_task;
pub mod project_manager;
pub mod read;
pub mod skill;
pub mod task;
pub mod tool_info;
pub mod tool_search;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use crate::config::AppConfig;
use crate::network::HttpClients;
use crate::skill::SkillRegistry;
use crate::tool::{DeferredToolCategory, ProjectDirService, Tool, ToolRegistry};
use orchestrate_hook::OrchestrateTaskHookSlot;
use std::sync::Arc;

async fn register_as_deferred(registry: &ToolRegistry, tool: Box<dyn Tool>, category: DeferredToolCategory) {
    let def = tool.definition();
    let tool: Arc<dyn Tool> = Arc::from(tool);
    let factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync> = Box::new(move || tool.clone());
    registry
        .register_deferred_with_category(def.name, def.description, def.input_schema, factory, category)
        .await;
}

/// Registers all built-in tools into the provided `ToolRegistry`.
///
/// 返回 `OrchestrateTaskHookSlot`：调用方可把该 slot 传给 `AgentApplicationImpl`
/// 用于后续注入 `OrchestrateTaskPromptHook`（外部宿主如 zero 用此前置注入
/// 子 Agent prompt 上下文）。
pub async fn register_builtin_tools(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
) -> OrchestrateTaskHookSlot {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        project_dir_service,
        http_clients,
        None,
    )
    .await
}

/// Registers built-in tools with subagent execution capabilities.
/// 返回值含义见 [`register_builtin_tools`]。
pub async fn register_builtin_tools_with_services(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    agent_services: Option<agent::AgentToolServices>,
) -> OrchestrateTaskHookSlot {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        project_dir_service,
        http_clients,
        agent_services,
    )
    .await
}

async fn register_builtin_tools_inner(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    agent_services: Option<agent::AgentToolServices>,
) -> OrchestrateTaskHookSlot {
    let shared_agent_tool = Arc::new(agent::AgentTool::new(config.clone(), agent_services));

    // Always-on: core tools available every turn
    registry
        .register(Box::new(bash::BashTool::new(&config.tool.bash)))
        .await;
    registry.register(Box::new(read::ReadTool::new(None))).await;
    registry.register(Box::new(write::WriteTool::new(None))).await;
    registry.register(Box::new(edit::EditTool::new(None))).await;
    registry.register(Box::new(tool_info::ToolInfoTool {})).await;
    registry.register(Box::new(tool_search::ToolSearchTool {})).await;
    registry
        .register(Box::new(skill::SkillTool::new(skill_registry.clone())))
        .await;
    registry
        .register(Box::new(task::TaskCreateTool::new(task_store.clone())))
        .await;
    registry
        .register(Box::new(task::TaskListTool::new(task_store.clone())))
        .await;
    registry
        .register(Box::new(task::TaskUpdateTool::new(task_store.clone())))
        .await;

    // Deferred: activated on demand via ToolSearch
    register_as_deferred(
        registry,
        Box::new(web_search::WebSearchTool::with_client(
            &config.search,
            http_clients.web.clone(),
        )),
        DeferredToolCategory::Search,
    )
    .await;
    register_as_deferred(
        registry,
        Box::new(web_fetch::WebFetchTool::with_client(http_clients.web.clone())),
        DeferredToolCategory::Search,
    )
    .await;
    register_as_deferred(
        registry,
        Box::new(project_manager::ProjectManagerTool::new(project_dir_service)),
        DeferredToolCategory::System,
    )
    .await;
    let orchestrate_tool = orchestrate_task::OrchestrateTaskTool::new(shared_agent_tool.clone());
    let prompt_hook_slot = orchestrate_tool.prompt_hook_slot();
    register_as_deferred(registry, Box::new(orchestrate_tool), DeferredToolCategory::System).await;

    prompt_hook_slot
}

#[cfg(test)]
mod tests {
    use super::register_builtin_tools;
    use crate::config::AppConfig;
    use crate::network::HttpClients;
    use crate::skill::SkillRegistry;
    use crate::tool::{ToolRegistry, UnavailableProjectDirService};
    use std::sync::Arc;

    async fn make_registry() -> ToolRegistry {
        let registry = ToolRegistry::new();
        register_builtin_tools(
            &registry,
            &AppConfig::new("D:/config".into()),
            super::task::TaskStoreHandle::new(super::task::TaskStore::default()),
            Arc::new(SkillRegistry::new()),
            Arc::new(UnavailableProjectDirService::new("unavailable")),
            &HttpClients::new().expect("http clients should build"),
        )
        .await;
        registry
    }

    #[tokio::test]
    async fn always_on_tools_are_loaded() {
        let registry = make_registry().await;
        for tool_name in [
            "Bash",
            "Read",
            "Write",
            "Edit",
            "ToolInfo",
            "ToolSearch",
            "Skill",
            "TaskCreate",
            "TaskList",
            "TaskUpdate",
        ] {
            assert!(
                registry.has_loaded_tool(tool_name).await,
                "always-on tool '{}' should be loaded",
                tool_name
            );
        }
    }

    #[tokio::test]
    async fn deferred_tools_are_registered_but_not_loaded() {
        let registry = make_registry().await;
        for tool_name in ["WebSearch", "WebFetch", "ProjectManager", "OrchestrateTask"] {
            let meta = registry.tool_metadata("s1", tool_name).await;
            assert!(meta.is_some(), "deferred tool '{}' should be discoverable", tool_name);
            assert!(
                meta.unwrap().deferred,
                "tool '{}' should be marked as deferred",
                tool_name
            );
        }
    }

    #[tokio::test]
    async fn turn_view_separates_loaded_and_deferred() {
        let registry = make_registry().await;
        let view = registry.get_turn_view("s1", true, true, true).await;

        let loaded_names: Vec<_> = view.loaded.iter().map(|d| d.name.as_str()).collect();
        assert!(loaded_names.contains(&"Bash"), "Bash should be in loaded set");
        assert!(loaded_names.contains(&"Skill"), "Skill should be in loaded set");
        assert!(
            !loaded_names.contains(&"WebSearch"),
            "WebSearch should not be in loaded set"
        );

        let deferred_names: Vec<_> = view.deferred.iter().map(|d| d.name.as_str()).collect();
        assert!(
            deferred_names.contains(&"WebSearch"),
            "WebSearch should be in deferred set"
        );
    }
}
