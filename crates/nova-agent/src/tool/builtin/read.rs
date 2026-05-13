use crate::tool::read_cache::ReadRange;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use tokio::fs;

const DEFAULT_OFFSET: usize = 1;
const DEFAULT_LIMIT: usize = 2000;
const MAX_LIMIT: usize = 2000;
const REPEAT_SUMMARY_TRIGGER_LINES: usize = 400;

pub struct ReadTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl ReadTool {
    pub fn new(root_dir: Option<std::path::PathBuf>) -> Self {
        Self { root_dir }
    }

    fn get_file_path<'a>(&self, input: &'a Value) -> Result<&'a str> {
        input["file_path"]
            .as_str()
            .or_else(|| input["path"].as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path'"))
    }

    fn validate_path(&self, path_str: &str) -> Result<std::path::PathBuf, ToolOutput> {
        let path = Path::new(path_str);
        if let Some(root) = &self.root_dir {
            let full_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            if path_str.contains("..") || !full_path.starts_with(root) {
                return Err(ToolOutput {
                    content: "Access denied: path is invalid or outside of workspace".to_string(),
                    is_error: true,
                });
            }
            Ok(full_path)
        } else {
            if !path.is_absolute() {
                return Err(ToolOutput {
                    content: format!("Error: 'file_path' must be an absolute path: {}", path_str),
                    is_error: true,
                });
            }
            Ok(path.to_path_buf())
        }
    }

    async fn read_text(
        &self,
        path: &Path,
        offset: usize,
        limit: usize,
        turn_read_state: Option<&tokio::sync::RwLock<crate::tool::read_cache::TurnReadState>>,
    ) -> Result<ToolOutput> {
        match fs::read_to_string(path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = offset.saturating_sub(1);
                if start >= lines.len() && !lines.is_empty() {
                    return Ok(ToolOutput {
                        content: format!("Offset {} is beyond file length ({} lines)", offset, lines.len()),
                        is_error: true,
                    });
                }

                let end = (start + limit).min(lines.len());
                let result_lines = &lines[start..end];
                let returned_line_count = end.saturating_sub(start);
                let has_more = end < lines.len();
                let next_offset = if has_more { end + 1 } else { end.max(1) };
                let canonical_path = path
                    .canonicalize()
                    .unwrap_or_else(|_| path.to_path_buf())
                    .to_string_lossy()
                    .to_string();
                let is_repeat_range = if let Some(state) = turn_read_state {
                    let guard = state.read().await;
                    guard
                        .file_state(&canonical_path)
                        .is_some_and(|f| f.ranges.iter().any(|r| r.offset_start == offset && r.offset_end == end))
                } else {
                    false
                };

                let mut output = String::new();

                if is_repeat_range {
                    output.push_str("Read repeat detected in current turn.\n");
                    output.push_str(&format!(
                        "file_path: {}\ntotal_lines: {}\nreturned_range: {}-{}\nhas_more: {}\nnext_offset: {}\n",
                        canonical_path,
                        lines.len(),
                        offset,
                        end,
                        has_more,
                        next_offset
                    ));
                    if returned_line_count >= REPEAT_SUMMARY_TRIGGER_LINES {
                        let head = result_lines
                            .iter()
                            .take(3)
                            .enumerate()
                            .map(|(i, line)| format!("{}\t{}", start + i + 1, line))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let tail = result_lines
                            .iter()
                            .rev()
                            .take(3)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .enumerate()
                            .map(|(i, line)| format!("{}\t{}", end - 2 + i, line))
                            .collect::<Vec<_>>()
                            .join("\n");
                        output.push_str("summary: this range has already been returned once in this turn. Returning summary to save context.\n");
                        output.push_str("head:\n");
                        output.push_str(&head);
                        output.push_str("\ntail:\n");
                        output.push_str(&tail);
                        output.push('\n');
                    } else {
                        output.push_str(
                            "suggestion: returning full content again since the requested range is short.\n\n",
                        );
                        for (i, line) in result_lines.iter().enumerate() {
                            let line_num = start + i + 1;
                            output.push_str(&format!("{}\t{}\n", line_num, line));
                        }
                    }
                } else {
                    output.push_str(&format!(
                        "file_path: {}\ntotal_lines: {}\nreturned_range: {}-{}\nhas_more: {}\nnext_offset: {}\n\n",
                        canonical_path,
                        lines.len(),
                        offset,
                        end,
                        has_more,
                        next_offset
                    ));
                    for (i, line) in result_lines.iter().enumerate() {
                        let line_num = start + i + 1;
                        output.push_str(&format!("{}\t{}\n", line_num, line));
                    }
                }

                if let Some(state) = turn_read_state {
                    let mut hasher = DefaultHasher::new();
                    result_lines.len().hash(&mut hasher);
                    if let Some(first) = result_lines.first() {
                        first.hash(&mut hasher);
                    }
                    if let Some(last) = result_lines.last() {
                        last.hash(&mut hasher);
                    }
                    state.write().await.record_range(
                        canonical_path,
                        ReadRange {
                            offset_start: offset,
                            offset_end: end,
                            returned_line_count,
                        },
                        hasher.finish(),
                    );
                }

                Ok(ToolOutput {
                    content: output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolOutput {
                content: format!("Failed to read file: {}", e),
                is_error: true,
            }),
        }
    }

    // Placeholder for multimodal support
    async fn read_image(&self, _path: &Path) -> Result<ToolOutput> {
        Ok(ToolOutput {
            content: "Image reading not yet supported in this version.".to_string(),
            is_error: true,
        })
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Read".to_string(),
            description: "Read the contents of a file. Supports paging, PDF (placeholder), and images.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "The absolute path to the file to read" },
                    "offset": { "type": "integer", "default": 1, "description": "1-based start line. Increase this offset to continue reading later lines." },
                    "limit": { "type": "integer", "default": 2000, "description": "Number of lines to read (default 2000, max 2000)" },
                    "pages": { "type": "string", "description": "Page range for PDF files (e.g., '1-5') - Currently not implemented" }
                },
                "required": ["file_path"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let file_path_str = self.get_file_path(&input)?;
        let offset = input["offset"].as_u64().unwrap_or(DEFAULT_OFFSET as u64) as usize;
        let limit = input["limit"]
            .as_u64()
            .unwrap_or(DEFAULT_LIMIT as u64)
            .min(MAX_LIMIT as u64) as usize;

        let full_path = match self.validate_path(file_path_str) {
            Ok(p) => p,
            Err(out) => return Ok(out),
        };

        if !full_path.exists() {
            return Ok(ToolOutput {
                content: format!("File not found: {}", file_path_str),
                is_error: true,
            });
        }

        // Track that this file has been read (after confirming existence).
        // Use canonicalized path so Edit/Write pre-read checks work with equivalent paths.
        if let Some(ctx) = &context {
            let canonical = full_path.canonicalize().unwrap_or_else(|_| full_path.clone());
            ctx.read_files
                .lock()
                .await
                .insert(canonical.to_string_lossy().to_string());
        }

        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" => self.read_image(&full_path).await,
            // Add other types here
            _ => {
                let turn_read_state = context.as_ref().and_then(|ctx| ctx.turn_read_state.as_deref());
                self.read_text(&full_path, offset, limit, turn_read_state).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReadTool;
    use crate::prompt::EnvironmentSnapshot;
    use crate::tool::read_cache::TurnReadState;
    use crate::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::{mpsc, Mutex, RwLock};

    fn build_context(state: Arc<RwLock<TurnReadState>>) -> ToolContext {
        build_context_with_read_files(state, Arc::new(Mutex::new(HashSet::new())))
    }

    fn build_context_with_read_files(
        state: Arc<RwLock<TurnReadState>>,
        read_files: Arc<Mutex<HashSet<String>>>,
    ) -> ToolContext {
        let (event_tx, _event_rx) = mpsc::channel(4);
        ToolContext {
            event_tx,
            tool_use_id: "read-tool-test".to_string(),
            session_id: "read-tool-session".to_string(),
            task_store: None,
            skill_registry: None,
            read_files,
            turn_read_state: Some(state),
            environment: Some(EnvironmentSnapshot {
                config_dir: "D:/git/zero-nova".to_string(),
                project_dir: None,
                platform: "windows".to_string(),
                shell: "powershell".to_string(),
                git_branch: None,
                git_status_summary: None,
                recent_commits: None,
                model_id: None,
                current_date: "2026-05-09".to_string(),
            }),
            shared_environment: None,
            cancellation_token: None,
            visible_tool_names: Arc::new(std::collections::HashSet::new()),
        }
    }

    #[tokio::test]
    async fn repeat_range_is_detected_within_same_turn() {
        let temp = tempdir().expect("create tempdir");
        let file_path = temp.path().join("a.txt");
        tokio::fs::write(&file_path, "line1\nline2\nline3\n")
            .await
            .expect("write file");

        let tool = ReadTool::new(Some(temp.path().to_path_buf()));
        let state = Arc::new(RwLock::new(TurnReadState::default()));
        let ctx = build_context(state);
        let absolute = file_path.to_string_lossy().to_string();

        let first = tool
            .execute(
                json!({"file_path": absolute, "offset": 1, "limit": 2}),
                Some(ctx.clone()),
            )
            .await
            .expect("first read");
        assert!(!first.is_error);
        assert!(first.content.contains("returned_range: 1-2"));

        let second = tool
            .execute(
                json!({"file_path": file_path.to_string_lossy().to_string(), "offset": 1, "limit": 2}),
                Some(ctx),
            )
            .await
            .expect("second read");
        assert!(!second.is_error);
        assert!(second.content.contains("Read repeat detected in current turn."));
    }

    #[tokio::test]
    async fn repeat_range_is_detected_for_canonicalized_equivalent_paths() {
        let temp = tempdir().expect("create tempdir");
        let nested = temp.path().join("nested");
        tokio::fs::create_dir_all(&nested).await.expect("create nested dir");
        let file_path = nested.join("b.txt");
        tokio::fs::write(&file_path, "x\ny\nz\n").await.expect("write file");

        let tool = ReadTool::new(Some(temp.path().to_path_buf()));
        let state = Arc::new(RwLock::new(TurnReadState::default()));
        let ctx = build_context(state);

        let relative = std::path::PathBuf::from("nested").join("b.txt");
        let absolute = file_path.to_string_lossy().to_string();

        let _ = tool
            .execute(
                json!({"file_path": relative.to_string_lossy().to_string(), "offset": 1, "limit": 2}),
                Some(ctx.clone()),
            )
            .await
            .expect("first read canonical");

        let second = tool
            .execute(json!({"file_path": absolute, "offset": 1, "limit": 2}), Some(ctx))
            .await
            .expect("second read equivalent path");
        assert!(
            second.content.contains("Read repeat detected in current turn."),
            "unexpected second output: {}",
            second.content
        );
    }

    #[tokio::test]
    async fn repeat_detection_does_not_leak_across_turn_states() {
        let temp = tempdir().expect("create tempdir");
        let file_path = temp.path().join("turn-isolation.txt");
        tokio::fs::write(&file_path, "a\nb\nc\nd\n").await.expect("write file");

        let tool = ReadTool::new(Some(temp.path().to_path_buf()));
        let shared_read_files = Arc::new(Mutex::new(HashSet::new()));
        let turn1 = Arc::new(RwLock::new(TurnReadState::default()));
        let turn2 = Arc::new(RwLock::new(TurnReadState::default()));
        let path = file_path.to_string_lossy().to_string();

        let first_turn_first = tool
            .execute(
                json!({"file_path": path, "offset": 1, "limit": 2}),
                Some(build_context_with_read_files(turn1.clone(), shared_read_files.clone())),
            )
            .await
            .expect("first turn first read");
        assert!(!first_turn_first
            .content
            .contains("Read repeat detected in current turn."));

        let first_turn_second = tool
            .execute(
                json!({"file_path": file_path.to_string_lossy().to_string(), "offset": 1, "limit": 2}),
                Some(build_context_with_read_files(turn1, shared_read_files.clone())),
            )
            .await
            .expect("first turn second read");
        assert!(first_turn_second
            .content
            .contains("Read repeat detected in current turn."));

        let second_turn_first = tool
            .execute(
                json!({"file_path": file_path.to_string_lossy().to_string(), "offset": 1, "limit": 2}),
                Some(build_context_with_read_files(turn2, shared_read_files)),
            )
            .await
            .expect("second turn first read");
        assert!(
            !second_turn_first
                .content
                .contains("Read repeat detected in current turn."),
            "turn state leaked across turns: {}",
            second_turn_first.content
        );
    }
}
