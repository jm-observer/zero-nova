use std::path::{Path, PathBuf};

pub(super) async fn normalize_project_dir(path: &Path) -> PathBuf {
    match tokio::fs::canonicalize(path).await {
        Ok(canonical) => canonical,
        Err(err) => {
            log::warn!(
                "Failed to canonicalize project_dir '{}': {}. Using raw path.",
                path.display(),
                err
            );
            path.to_path_buf()
        }
    }
}

pub(super) fn sync_last_turn_prompt_preview(
    last_turn_snapshot: Option<&mut super::super::control::LastTurnSnapshot>,
    prompt_base_override: &str,
) -> bool {
    let Some(snapshot) = last_turn_snapshot else {
        return false;
    };
    let Some(preview) = snapshot.prompt_preview.as_mut() else {
        return false;
    };
    let Some(preview_obj) = preview.as_object_mut() else {
        return false;
    };

    preview_obj.insert(
        "system_prompt".to_string(),
        serde_json::Value::String(prompt_base_override.to_string()),
    );
    preview_obj.insert(
        "rendered_prompt".to_string(),
        serde_json::Value::String(prompt_base_override.to_string()),
    );
    true
}

pub(super) fn normalize_generated_title(raw: &str) -> String {
    let single_line = raw
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    let mut result = String::with_capacity(single_line.len());
    for ch in single_line.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        result.push(ch);
        if result.chars().count() >= 40 {
            break;
        }
    }
    result
}
