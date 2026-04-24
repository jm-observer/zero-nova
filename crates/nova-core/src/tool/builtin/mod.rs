pub mod agent;
pub mod bash;
pub mod edit;
pub mod read;
pub mod skill;
pub mod task;
pub mod tool_search;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use crate::tool::ToolRegistry;
use std::sync::Arc;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(
    registry: &mut ToolRegistry,
    config: &crate::config::AppConfig,
    task_store: std::sync::Arc<tokio::sync::Mutex<task::TaskStore>>,
    skill_registry: std::sync::Arc<crate::skill::SkillRegistry>,
    tool_whitelist: Option<&[String]>,
) {
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
        registry.register(Box::new(agent::AgentTool::new(config.clone())));
    }
    if is_tool_enabled(tool_whitelist, "WebSearch") {
        registry.register(Box::new(web_search::WebSearchTool::new(&config.search)));
    }
    if is_tool_enabled(tool_whitelist, "WebFetch") {
        registry.register(Box::new(web_fetch::WebFetchTool::new()));
    }

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
        _ => &[],
    }
}
