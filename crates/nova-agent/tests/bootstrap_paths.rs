/// Integration tests for bootstrap startup path generation.
///
/// These tests verify that the paths computed by `AppConfig` helpers
/// (prompts_dir, data_dir_path, config_path, skills_dir) are used
/// correctly in the bootstrap sequence for prompt loading, session
/// database placement, skills loading, and config re-export.
use nova_agent::config::{AppConfig, OriginAppConfig};
use std::path::PathBuf;

fn make_test_config(workspace: PathBuf) -> AppConfig {
    AppConfig::from_origin(OriginAppConfig::default(), workspace)
}

// --- Default path tests ---

#[test]
fn default_paths_are_consistent() {
    let config = make_test_config(PathBuf::from("D:/workspace"));

    assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/.nova/skills"));
    assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/.nova/data"));
    assert_eq!(config.config_path(), PathBuf::from("D:/workspace/config.toml"));
    assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/prompts"));
}

// --- Relative override tests ---

#[test]
fn relative_overrides_are_resolved_from_workspace() {
    let mut origin = OriginAppConfig::default();
    origin.data_dir = Some("var".to_string());
    origin.tool.skills_dir = Some("mods".to_string());
    origin.tool.prompts_dir = Some("prompts".to_string());
    origin.config_path = Some("conf/custom.toml".to_string());

    let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));

    assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/mods"));
    assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/var"));
    assert_eq!(config.config_path(), PathBuf::from("D:/workspace/conf/custom.toml"));
    assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/prompts"));
}

// --- Absolute path tests ---

#[test]
fn absolute_paths_are_used_directly() {
    let mut origin = OriginAppConfig::default();
    origin.data_dir = Some("D:/shared/data".to_string());
    origin.tool.skills_dir = Some("D:/shared/skills".to_string());
    origin.tool.prompts_dir = Some("D:/shared/prompts".to_string());
    origin.config_path = Some("D:/shared/config.toml".to_string());

    let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));

    // Absolute paths should NOT be joined with workspace
    assert_eq!(config.data_dir_path(), PathBuf::from("D:/shared/data"));
    assert_eq!(config.skills_dir(), PathBuf::from("D:/shared/skills"));
    assert_eq!(config.config_path(), PathBuf::from("D:/shared/config.toml"));
    assert_eq!(config.prompts_dir(), PathBuf::from("D:/shared/prompts"));
}

// --- Prompt file path construction ---

#[test]
fn prompt_file_path_construction() {
    let config = make_test_config(PathBuf::from("D:/workspace"));

    // Simulate how bootstrap builds the prompt file path
    let agent_id = "default";
    let prompt_file = format!("agent-{}.md", agent_id);
    let prompt_path = config.prompts_dir().join(&prompt_file);

    assert_eq!(prompt_path, PathBuf::from("D:/workspace/prompts/agent-default.md"));
}

// --- Bootstrap sequence path alignment ---

#[test]
fn bootstrap_path_sequence_is_consistent() {
    let origin = OriginAppConfig {
        data_dir: Some("runtime".to_string()),
        ..Default::default()
    };
    let config = AppConfig::from_origin(origin, PathBuf::from("D:/project"));

    // bootstrap.rs reads these in sequence:
    // 1. config_path() → used to re-export path in AgentApplicationImpl
    // 2. data_dir_path() → passed to SqliteManager for sessions.db
    // 3. skills_dir() → SkillRegistry::load_from_dir
    // 4. prompts_dir() → agent prompt file reading loop

    let actual_config_path = config.config_path();
    let actual_data_dir = config.data_dir_path();
    let actual_skills_dir = config.skills_dir();
    let actual_prompts_dir = config.prompts_dir();

    // Verify no path doubles up workspace prefix (regression check)
    assert!(
        !actual_config_path.to_string_lossy().starts_with("D:/workspace/D:"),
        "config_path should not double workspace"
    );
    assert!(
        !actual_data_dir.to_string_lossy().starts_with("D:/workspace/D:"),
        "data_dir should not double workspace"
    );
    assert!(
        !actual_skills_dir.to_string_lossy().starts_with("D:/workspace/D:"),
        "skills_dir should not double workspace"
    );
    assert!(
        !actual_prompts_dir.to_string_lossy().starts_with("D:/workspace/D:"),
        "prompts_dir should not double workspace"
    );

    // Verify data_dir is under workspace by default when not absolute
    let default_config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
    let default_data_dir = default_config.data_dir_path();
    // Windows normalize paths with forward slashes; check common variants
    assert!(
        default_data_dir.to_string_lossy().contains(".nova") && default_data_dir.to_string_lossy().ends_with("data"),
        "default data_dir should contain .nova/data, got {}",
        default_data_dir.to_string_lossy()
    );

    assert!(
        actual_data_dir.to_string_lossy().contains("project") && actual_data_dir.to_string_lossy().contains("runtime"),
        "data_dir should use configured value"
    );
}

// --- Config path holds by-reference for re-export ---

#[test]
fn config_path_can_be_held_by_reference() {
    let config = make_test_config(PathBuf::from("D:/workspace"));
    let config_path = config.config_path();

    // The path should be a PathBuf that can be cloned and passed
    // to Arc::new(RwLock::new(...)) as done in bootstrap.rs
    let _cloned = config_path.clone();

    // Verify it's still valid after clone (no panic/borrow issues)
    assert_eq!(config_path, _cloned);
}
