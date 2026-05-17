pub mod executor;
pub mod schema;

use crate::tool::{DeferredToolCategory, ToolRegistry};
use anyhow::{Context, Result};
use schema::{ExternalToolDefinition, ToolFile};
use std::path::Path;
use std::sync::Arc;

/// Scans a directory for external tool definitions and registers them as deferred tools.
pub async fn register_external_tools(registry: &ToolRegistry, tools_dir: &Path) {
    let definitions = match load_tools_from_dir(tools_dir) {
        Ok(defs) => defs,
        Err(e) => {
            log::warn!("failed to load external tools from {}: {}", tools_dir.display(), e);
            return;
        }
    };
    if definitions.is_empty() {
        return;
    }
    log::info!(
        "loaded {} external tool definition(s) from {}",
        definitions.len(),
        tools_dir.display()
    );
    for def in definitions {
        let name = def.name.clone();
        let description = def.description.clone();
        let input_schema = def.input_schema.clone();
        let def = Arc::new(def);
        let factory: Box<dyn Fn() -> Arc<dyn crate::tool::Tool> + Send + Sync> =
            Box::new(move || Arc::new(executor::ExternalCommandTool::from_definition((*def).clone())));
        registry
            .register_deferred_with_category(name, description, input_schema, factory, DeferredToolCategory::System)
            .await;
    }
}

pub fn load_tool_file(path: &Path) -> Result<Vec<ExternalToolDefinition>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("failed to read tool file: {}", path.display()))?;
    let tool_file: ToolFile =
        toml::from_str(&content).with_context(|| format!("failed to parse tool file: {}", path.display()))?;
    Ok(tool_file.tools.into_iter().map(|spec| spec.into_definition()).collect())
}

pub fn load_tools_from_dir(dir: &Path) -> Result<Vec<ExternalToolDefinition>> {
    let mut definitions = Vec::new();
    if !dir.exists() {
        return Ok(definitions);
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("failed to read tools directory: {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            for sub_entry in std::fs::read_dir(&path)? {
                let sub_path = sub_entry?.path();
                if sub_path.extension().is_some_and(|ext| ext == "toml") {
                    match load_tool_file(&sub_path) {
                        Ok(defs) => definitions.extend(defs),
                        Err(e) => log::warn!("skipping tool file {}: {}", sub_path.display(), e),
                    }
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            match load_tool_file(&path) {
                Ok(defs) => definitions.extend(defs),
                Err(e) => log::warn!("skipping tool file {}: {}", path.display(), e),
            }
        }
    }
    Ok(definitions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_tools_from_dir_nested() {
        let dir = TempDir::new().unwrap();
        let tool_dir = dir.path().join("my-tool");
        fs::create_dir_all(&tool_dir).unwrap();
        fs::write(
            tool_dir.join("my-tool.toml"),
            r#"
[[tools]]
name = "my-tool"
description = "A test tool"
type = "command"
command = "my-tool"
cwd = false

[[tools.parameters]]
name = "input"
description = "Input value"
type = "string"
required = true
arg = ["--input"]
"#,
        )
        .unwrap();

        let defs = load_tools_from_dir(dir.path()).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "my-tool");
        assert_eq!(defs[0].execution.command, "my-tool");
    }

    #[test]
    fn test_load_tools_from_dir_flat() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("flat-tool.toml"),
            r#"
[[tools]]
name = "flat-tool"
description = "A flat tool"
type = "command"
command = "flat"
cwd = false
"#,
        )
        .unwrap();

        let defs = load_tools_from_dir(dir.path()).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "flat-tool");
    }

    #[test]
    fn test_load_tools_from_nonexistent_dir() {
        let defs = load_tools_from_dir(Path::new("/nonexistent/path")).unwrap();
        assert!(defs.is_empty());
    }

    #[test]
    fn test_load_tools_skips_invalid_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("bad.toml"), "not valid toml [[[").unwrap();
        fs::write(
            dir.path().join("good.toml"),
            r#"
[[tools]]
name = "good"
description = "Good tool"
type = "command"
command = "good"
cwd = false
"#,
        )
        .unwrap();

        let defs = load_tools_from_dir(dir.path()).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "good");
    }
}
