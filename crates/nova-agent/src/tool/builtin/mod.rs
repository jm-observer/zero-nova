pub mod agent;
pub mod bash;
pub mod edit;
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
use crate::tool::{ProjectDirService, ToolRegistry};
use std::sync::Arc;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub async fn register_builtin_tools(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
) {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        project_dir_service,
        http_clients,
        None,
    )
    .await;
}

/// Registers built-in tools with subagent execution capabilities.
pub async fn register_builtin_tools_with_services(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    agent_services: Option<agent::AgentToolServices>,
) {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        project_dir_service,
        http_clients,
        agent_services,
    )
    .await;
}

async fn register_builtin_tools_inner(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    agent_services: Option<agent::AgentToolServices>,
) {
    let shared_agent_tool = Arc::new(agent::AgentTool::new(config.clone(), agent_services));

    registry
        .register(Box::new(bash::BashTool::new(&config.tool.bash)))
        .await;
    registry.register(Box::new(read::ReadTool::new(None))).await;
    registry.register(Box::new(write::WriteTool::new(None))).await;
    registry.register(Box::new(edit::EditTool::new(None))).await;
    registry.register(Box::new((*shared_agent_tool).clone())).await;
    registry
        .register(Box::new(web_search::WebSearchTool::with_client(
            &config.search,
            http_clients.web.clone(),
        )))
        .await;
    registry
        .register(Box::new(web_fetch::WebFetchTool::with_client(http_clients.web.clone())))
        .await;
    registry
        .register(Box::new(project_manager::ProjectManagerTool::new(project_dir_service)))
        .await;
    registry
        .register(Box::new(orchestrate_task::OrchestrateTaskTool::new(
            shared_agent_tool.clone(),
        )))
        .await;

    // ToolInfo is always registered as a loaded tool (schema lookup infrastructure)
    registry.register(Box::new(tool_info::ToolInfoTool {})).await;

    let skill_registry_for_skill = skill_registry.clone();
    registry
        .register_deferred(
            "Skill".to_string(),
            "Loads and injects specialized skills into the current session.".to_string(),
            skill::SkillTool::input_schema(),
            Box::new(move || Arc::new(skill::SkillTool::new(skill_registry_for_skill.clone()))),
        )
        .await;

    let task_store_for_create = task_store.clone();
    registry
        .register_deferred(
            "TaskCreate".to_string(),
            "Creates a new task in the session's task store.".to_string(),
            task::TaskCreateTool::input_schema(),
            Box::new(move || Arc::new(task::TaskCreateTool::new(task_store_for_create.clone()))),
        )
        .await;

    let task_store_for_list = task_store.clone();
    registry
        .register_deferred(
            "TaskList".to_string(),
            "Lists all tasks in the session's task store.".to_string(),
            task::TaskListTool::input_schema(),
            Box::new(move || Arc::new(task::TaskListTool::new(task_store_for_list.clone()))),
        )
        .await;

    let task_store_for_update = task_store;
    registry
        .register_deferred(
            "TaskUpdate".to_string(),
            "Updates an existing task.".to_string(),
            task::TaskUpdateTool::input_schema(),
            Box::new(move || Arc::new(task::TaskUpdateTool::new(task_store_for_update.clone()))),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::register_builtin_tools;
    use crate::config::AppConfig;
    use crate::network::HttpClients;
    use crate::skill::SkillRegistry;
    use crate::tool::{ToolRegistry, UnavailableProjectDirService};
    use std::sync::Arc;

    #[tokio::test]
    async fn orchestrate_task_is_registered_by_default() {
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

        assert!(registry.has_loaded_tool("OrchestrateTask").await);
    }

    #[tokio::test]
    async fn unified_runtime_exposes_shared_tool_set() {
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

        for tool_name in [
            "Bash",
            "Read",
            "Write",
            "Edit",
            "Agent",
            "WebSearch",
            "WebFetch",
            "ProjectManager",
            "OrchestrateTask",
            "ToolInfo",
        ] {
            assert!(
                registry.has_loaded_tool(tool_name).await,
                "tool '{}' should be loaded in the unified runtime",
                tool_name
            );
        }

        for tool_name in ["Skill", "TaskCreate", "TaskList", "TaskUpdate"] {
            assert!(
                registry.tool_metadata(tool_name).await.is_some(),
                "tool '{}' should be present in runtime metadata",
                tool_name
            );
        }
    }
}
