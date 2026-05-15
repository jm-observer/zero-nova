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

/// Wiring parameters for built-in tools that require subagent execution capabilities.
pub struct BuiltinToolWiring {
    pub services: Option<agent::AgentToolServices>,
}

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    tool_whitelist: Option<&[String]>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
) {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        tool_whitelist,
        project_dir_service,
        http_clients,
        BuiltinToolWiring { services: None },
    );
}

/// Registers built-in tools with subagent execution capabilities.
pub fn register_builtin_tools_with_services(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    tool_whitelist: Option<&[String]>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    wiring: BuiltinToolWiring,
) {
    register_builtin_tools_inner(
        registry,
        config,
        task_store,
        skill_registry,
        tool_whitelist,
        project_dir_service,
        http_clients,
        wiring,
    );
}

fn register_builtin_tools_inner(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: task::TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    tool_whitelist: Option<&[String]>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    wiring: BuiltinToolWiring,
) {
    let shared_agent_tool =
        if is_tool_enabled(tool_whitelist, "Agent") || is_tool_explicitly_enabled(tool_whitelist, "OrchestrateTask") {
            Some(Arc::new(agent::AgentTool::new(config.clone(), wiring.services)))
        } else {
            None
        };

    if is_tool_enabled(tool_whitelist, "Bash") {
        registry.register(Box::new(bash::BashTool::new(&config.tool.bash)));
    }
    if is_tool_enabled(tool_whitelist, "Read") {
        registry.register(Box::new(read::ReadTool::new(None)));
    }
    if is_tool_enabled(tool_whitelist, "Write") {
        registry.register(Box::new(write::WriteTool::new(None)));
    }
    if is_tool_enabled(tool_whitelist, "Edit") {
        registry.register(Box::new(edit::EditTool::new(None)));
    }
    if is_tool_enabled(tool_whitelist, "Agent") {
        if let Some(agent_tool) = &shared_agent_tool {
            registry.register(Box::new((**agent_tool).clone()));
        }
    }
    if is_tool_enabled(tool_whitelist, "WebSearch") {
        registry.register(Box::new(web_search::WebSearchTool::with_client(
            &config.search,
            http_clients.web.clone(),
        )));
    }
    if is_tool_enabled(tool_whitelist, "WebFetch") {
        registry.register(Box::new(web_fetch::WebFetchTool::with_client(http_clients.web.clone())));
    }
    if is_tool_enabled(tool_whitelist, "ProjectManager") {
        registry.register(Box::new(project_manager::ProjectManagerTool::new(project_dir_service)));
    }
    if is_tool_explicitly_enabled(tool_whitelist, "OrchestrateTask") {
        if let Some(agent_tool) = &shared_agent_tool {
            registry.register(Box::new(orchestrate_task::OrchestrateTaskTool::new(agent_tool.clone())));
        }
    }

    // ToolInfo is always registered as a loaded tool (schema lookup infrastructure)
    registry.register(Box::new(tool_info::ToolInfoTool {}));

    let skill_registry_for_skill = skill_registry.clone();
    if is_tool_enabled(tool_whitelist, "Skill") {
        registry.register_deferred(
            "Skill".to_string(),
            "Loads and injects specialized skills into the current session.".to_string(),
            skill::SkillTool::input_schema(),
            Box::new(move || Arc::new(skill::SkillTool::new(skill_registry_for_skill.clone()))),
        );
    }

    let task_store_for_create = task_store.clone();
    if is_tool_enabled(tool_whitelist, "TaskCreate") {
        registry.register_deferred(
            "TaskCreate".to_string(),
            "Creates a new task in the session's task store.".to_string(),
            task::TaskCreateTool::input_schema(),
            Box::new(move || Arc::new(task::TaskCreateTool::new(task_store_for_create.clone()))),
        );
    }

    let task_store_for_list = task_store.clone();
    if is_tool_enabled(tool_whitelist, "TaskList") {
        registry.register_deferred(
            "TaskList".to_string(),
            "Lists all tasks in the session's task store.".to_string(),
            task::TaskListTool::input_schema(),
            Box::new(move || Arc::new(task::TaskListTool::new(task_store_for_list.clone()))),
        );
    }

    let task_store_for_update = task_store;
    if is_tool_enabled(tool_whitelist, "TaskUpdate") {
        registry.register_deferred(
            "TaskUpdate".to_string(),
            "Updates an existing task.".to_string(),
            task::TaskUpdateTool::input_schema(),
            Box::new(move || Arc::new(task::TaskUpdateTool::new(task_store_for_update.clone()))),
        );
    }
}

/// Legacy tool names that map to their current canonical names.
/// Kept for backwards compatibility with existing agent configurations.
fn is_tool_enabled(tool_whitelist: Option<&[String]>, tool_name: &str) -> bool {
    match tool_whitelist {
        None => true,
        Some(whitelist) => {
            let legacy_aliases = legacy_tool_names(tool_name);
            whitelist
                .iter()
                .any(|name| name == tool_name || legacy_aliases.iter().any(|alias| name == alias))
        }
    }
}

fn is_tool_explicitly_enabled(tool_whitelist: Option<&[String]>, tool_name: &str) -> bool {
    let Some(whitelist) = tool_whitelist else {
        return false;
    };

    let legacy_aliases = legacy_tool_names(tool_name);
    whitelist
        .iter()
        .any(|name| name == tool_name || legacy_aliases.iter().any(|alias| name == alias))
}

/// Return the set of legacy names that map to the given tool name.
fn legacy_tool_names(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        "Bash" => &["bash", "shell"],
        "Read" => &["file_read", "read", "open_file"],
        "Write" => &["file_write", "write", "create_file"],
        "Edit" => &["file_edit", "edit"],
        "Agent" => &["subagent", "agent_sub"],
        "WebSearch" => &["web_search", "search"],
        "WebFetch" => &["web_fetch", "fetch"],
        "Skill" => &["skill"],
        "TaskCreate" => &["task_create", "create_task"],
        "TaskList" => &["task_list", "list_tasks"],
        "TaskUpdate" => &["task_update", "update_task", "task"],
        "OrchestrateTask" => &["orchestrate_task"],
        _ => &[],
    }
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
    async fn orchestrate_task_is_hidden_without_explicit_whitelist() {
        let registry = ToolRegistry::new();
        register_builtin_tools(
            &registry,
            &AppConfig::new("D:/config".into()),
            super::task::TaskStoreHandle::new(super::task::TaskStore::default()),
            Arc::new(SkillRegistry::new()),
            None,
            Arc::new(UnavailableProjectDirService::new("unavailable")),
            &HttpClients::new().expect("http clients should build"),
        );

        assert!(!registry.has_loaded_tool("OrchestrateTask").await);
    }

    #[tokio::test]
    async fn orchestrate_task_is_visible_when_whitelisted() {
        let registry = ToolRegistry::new();
        let whitelist = vec!["OrchestrateTask".to_string()];
        register_builtin_tools(
            &registry,
            &AppConfig::new("D:/config".into()),
            super::task::TaskStoreHandle::new(super::task::TaskStore::default()),
            Arc::new(SkillRegistry::new()),
            Some(&whitelist),
            Arc::new(UnavailableProjectDirService::new("unavailable")),
            &HttpClients::new().expect("http clients should build"),
        );

        assert!(registry.has_loaded_tool("OrchestrateTask").await);
    }
}
