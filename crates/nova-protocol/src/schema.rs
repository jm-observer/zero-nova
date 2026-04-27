use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const FIXTURES: &[(&str, &str)] = &[
    ("chat.json", include_str!("../../../schemas/fixtures/chat.json")),
    (
        "chat_complete.json",
        include_str!("../../../schemas/fixtures/chat_complete.json"),
    ),
    ("error.json", include_str!("../../../schemas/fixtures/error.json")),
    (
        "invalid_chat_missing_input.json",
        include_str!("../../../schemas/fixtures/invalid_chat_missing_input.json"),
    ),
    (
        "invalid_error_missing_code.json",
        include_str!("../../../schemas/fixtures/invalid_error_missing_code.json"),
    ),
    (
        "invalid_welcome_missing_optional_field.json",
        include_str!("../../../schemas/fixtures/invalid_welcome_missing_optional_field.json"),
    ),
    (
        "progress_event.json",
        include_str!("../../../schemas/fixtures/progress_event.json"),
    ),
    (
        "skill_activated.json",
        include_str!("../../../schemas/fixtures/skill_activated.json"),
    ),
    (
        "task_status_changed.json",
        include_str!("../../../schemas/fixtures/task_status_changed.json"),
    ),
    ("welcome.json", include_str!("../../../schemas/fixtures/welcome.json")),
];

const REQUIRED_SCHEMA_FILES: &[&str] = &[
    "schemas/registry.json",
    "schemas/root/schema-root.json",
    "schemas/domains/chat/chat-payload.schema.json",
    "schemas/domains/chat/chat-complete-payload.schema.json",
    "schemas/domains/chat/progress-event.schema.json",
    "schemas/domains/gateway/gateway-message.schema.json",
    "schemas/domains/gateway/message-envelope.schema.json",
    "schemas/domains/session/session-create-request.schema.json",
    "schemas/domains/system/error-payload.schema.json",
    "schemas/domains/system/welcome-payload.schema.json",
];

pub fn export_repository_artifacts(root: &Path) -> Result<()> {
    sync_shared_fixtures(root)?;
    verify_schema_artifacts(root)?;
    write_domain_snapshot(root)?;
    Ok(())
}

fn sync_shared_fixtures(root: &Path) -> Result<()> {
    let fixture_dir = root.join("schemas").join("fixtures");
    fs::create_dir_all(&fixture_dir).with_context(|| format!("创建 fixture 目录失败: {}", fixture_dir.display()))?;

    for (name, content) in FIXTURES {
        let path = fixture_dir.join(name);
        write_if_changed(&path, content)?;
    }

    Ok(())
}

fn verify_schema_artifacts(root: &Path) -> Result<()> {
    for relative_path in REQUIRED_SCHEMA_FILES {
        let path = root.join(relative_path);
        if !path.is_file() {
            anyhow::bail!("缺少 schema 工件: {}", path.display());
        }
    }

    Ok(())
}

fn write_domain_snapshot(root: &Path) -> Result<()> {
    let domains_dir = root.join("schemas").join("domains");
    let mut files = collect_schema_files(&domains_dir)?;
    files.sort();

    let snapshot = files
        .into_iter()
        .map(|path| path.strip_prefix(root).unwrap_or(path.as_path()).display().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let snapshot_path = root.join("schemas").join("domains_snapshot.txt");
    write_if_changed(&snapshot_path, &format!("{}\n", snapshot))
}

fn collect_schema_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir).with_context(|| format!("读取目录失败: {}", dir.display()))? {
        let entry = entry.with_context(|| format!("读取目录项失败: {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(collect_schema_files(&path)?);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }

    Ok(files)
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if should_write {
        fs::write(path, content).with_context(|| format!("写入文件失败: {}", path.display()))?;
    }

    Ok(())
}
