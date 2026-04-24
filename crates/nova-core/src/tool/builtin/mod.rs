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

use crate::tool::{Tool, ToolRegistry};
use std::sync::Arc;

/// Registers all built-in tools into the provided `ToolRegistry`.
pub fn register_builtin_tools(
    registry: &mut ToolRegistry,
    config: &crate::config::AppConfig,
    task_store: std::sync::Arc<tokio::sync::Mutex<task::TaskStore>>,
    skill_registry: std::sync::Arc<crate::skill::SkillRegistry>,
) {
    registry.register(Box::new(bash::BashTool::new(&config.tool.bash)));
    registry.register(Box::new(read::ReadTool::new(None)));
    registry.register(Box::new(write::WriteTool::new(None)));
    registry.register(Box::new(edit::EditTool::new(None)));
    registry.register(Box::new(agent::AgentTool::new(config.clone())));
    registry.register(Box::new(web_search::WebSearchTool::new(&config.search)));
    registry.register(Box::new(web_fetch::WebFetchTool::new()));

    let skill_registry_for_skill = skill_registry.clone();
    registry.register_deferred(
        "Skill".to_string(),
        "Loads and injects specialized skills into the current session.".to_string(),
        skill::SkillTool::new(skill_registry.clone()).definition().input_schema,
        Box::new(move || Arc::new(skill::SkillTool::new(skill_registry_for_skill.clone()))),
    );

    let task_store_for_create = task_store.clone();
    registry.register_deferred(
        "TaskCreate".to_string(),
        "Creates a new task in the session's task store.".to_string(),
        task::TaskCreateTool::new(task_store.clone()).definition().input_schema,
        Box::new(move || Arc::new(task::TaskCreateTool::new(task_store_for_create.clone()))),
    );

    let task_store_for_list = task_store.clone();
    registry.register_deferred(
        "TaskList".to_string(),
        "Lists all tasks in the session's task store.".to_string(),
        task::TaskListTool::new(task_store.clone()).definition().input_schema,
        Box::new(move || Arc::new(task::TaskListTool::new(task_store_for_list.clone()))),
    );

    let task_store_for_update = task_store;
    registry.register_deferred(
        "TaskUpdate".to_string(),
        "Updates an existing task.".to_string(),
        task::TaskUpdateTool::new(task_store_for_update.clone())
            .definition()
            .input_schema,
        Box::new(move || Arc::new(task::TaskUpdateTool::new(task_store_for_update.clone()))),
    );
}
