use crate::prompt::templates::TemplateContext;
use std::collections::HashMap;
use std::path::Path;

/// 工作流阶段 prompt 集合。
pub struct WorkflowStagePrompts {
    stages: HashMap<String, String>,
}

impl WorkflowStagePrompts {
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse_workflow_stage_content(&content)
    }

    pub async fn load_from_file_async(path: &Path) -> anyhow::Result<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        Self::parse_workflow_stage_content(&content)
    }

    fn parse_workflow_stage_content(content: &str) -> anyhow::Result<Self> {
        let mut stages = HashMap::new();
        let mut current_stage: Option<String> = None;
        let mut current_content = String::new();
        let mut in_code_block = false;

        for line in content.lines() {
            if line.starts_with("## ") && !in_code_block {
                if let Some(stage) = current_stage.take() {
                    let trimmed = current_content.trim().to_string();
                    if !trimmed.is_empty() {
                        stages.insert(stage, trimmed);
                    }
                }
                current_stage = Some(line[3..].trim().to_string());
                current_content.clear();
            } else if line.starts_with("```") {
                in_code_block = !in_code_block;
            } else if in_code_block {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }

        if let Some(stage) = current_stage {
            let trimmed = current_content.trim().to_string();
            if !trimmed.is_empty() {
                stages.insert(stage, trimmed);
            }
        }

        Ok(Self { stages })
    }

    pub fn get(&self, stage: &str) -> Option<&str> {
        self.stages.get(stage).map(|s| s.as_str())
    }

    pub fn render(&self, stage: &str, vars: &HashMap<String, String>) -> Option<String> {
        self.get(stage).map(|template| TemplateContext::render(template, vars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_temp_dir(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("zero-nova-{}-{}", prefix, suffix));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn workflow_stage_prompts_loads_code_blocks_only() {
        let dir = create_temp_dir("workflow-prompts");
        let file = dir.join("workflow-stages.md");
        fs::write(
            &file,
            "## analyze\noutside\n```md\ninside {{topic}}\n```\n## idle\n```md\nidle prompt\n```",
        )
        .unwrap();

        let prompts = WorkflowStagePrompts::load_from_file(&file).unwrap();
        let mut vars = HashMap::new();
        vars.insert("topic".to_string(), "prompt".to_string());

        assert_eq!(prompts.render("analyze", &vars).as_deref(), Some("inside prompt"));
        assert_eq!(prompts.get("idle"), Some("idle prompt"));

        fs::remove_dir_all(dir).unwrap();
    }
}
