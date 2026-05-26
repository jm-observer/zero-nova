use crate::path_resolver::{resolve_path_ref, PathResolveError};
use crate::tool::{ToolContext, ToolOutput};
use serde_json::Value;
use std::path::Path;
pub(super) const NO_PROJECT_RELATIVE_PATH_ERROR: &str =
    "Current session has no project directory. Set a project before using relative paths.";
pub(super) fn preprocess_file_tool_input(
    tool_name: &str,
    input: &mut Value,
    context: Option<&ToolContext>,
) -> Result<(), ToolOutput> {
    let raw_path = input
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| input.get("path").and_then(Value::as_str))
        .ok_or_else(|| ToolOutput {
            content: "Missing 'file_path'".to_string(),
            is_error: true,
            child_session: None,
        })?;
    let Some(ctx) = context else {
        return Ok(());
    };
    let Some(env) = ctx.environment.as_ref() else {
        return Ok(());
    };
    let raw_path = Path::new(raw_path);
    if raw_path.is_absolute() {
        let require_exists = !matches!(tool_name, "Write");
        let resolved = resolve_path_ref(
            raw_path.to_string_lossy().as_ref(),
            Path::new("."),
            None,
            require_exists,
        )
        .map_err(|err| ToolOutput {
            content: format_path_resolve_error(tool_name, &err),
            is_error: true,
            child_session: None,
        })?;
        input["file_path"] = Value::String(resolved.target_path.to_string_lossy().to_string());
        return Ok(());
    }
    let Some(project_dir) = env.project_dir.as_deref() else {
        return Err(ToolOutput {
            content: NO_PROJECT_RELATIVE_PATH_ERROR.to_string(),
            is_error: true,
            child_session: None,
        });
    };
    let project_dir = Path::new(project_dir);
    let allowed_root = project_dir;
    let require_exists = !matches!(tool_name, "Write");
    let resolved = resolve_path_ref(
        raw_path.to_string_lossy().as_ref(),
        project_dir,
        Some(allowed_root),
        require_exists,
    )
    .map_err(|err| ToolOutput {
        content: format_path_resolve_error(tool_name, &err),
        is_error: true,
        child_session: None,
    })?;
    input["file_path"] = Value::String(resolved.target_path.to_string_lossy().to_string());
    Ok(())
}
fn format_path_resolve_error(tool_name: &str, err: &PathResolveError) -> String {
    match err {
        PathResolveError::InvalidPathSyntax { .. } => {
            format!("{} path resolution failed: {}", tool_name, err)
        }
        PathResolveError::PathNotFound { .. } => {
            format!("{} path resolution failed: {}", tool_name, err)
        }
        PathResolveError::PathAccessDenied { .. } => {
            format!("{} path resolution failed: {}", tool_name, err)
        }
    }
}
