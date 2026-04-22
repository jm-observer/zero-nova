use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Default)]
pub struct SkillRegistry {
    pub skills: Vec<Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<()> {
        let dir = dir.as_ref();
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.load_single_skill(&path)?;
            }
        }
        Ok(())
    }

    pub fn load_single_skill<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() {
            match self.parse_skill_file(&skill_md) {
                Ok(skill) => {
                    log::info!("Loaded skill: {} from {:?}", skill.name, path);
                    self.skills.push(skill);
                    Ok(())
                }
                Err(e) => Err(anyhow::anyhow!("Failed to parse skill at {:?}: {}", path, e)),
            }
        } else {
            Ok(())
        }
    }

    fn parse_skill_file(&self, path: &Path) -> Result<Skill> {
        let content = std::fs::read_to_string(path)?;
        let parts: Vec<&str> = content.split("---").collect();

        if parts.len() < 3 {
            // 可能是没有 frontmatter 的简单 markdown
            return Ok(Skill {
                name: path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                description: String::new(),
                body: content,
                path: path.parent().unwrap().to_path_buf(),
            });
        }

        let frontmatter = parts[1];
        let body = parts[2..].join("---");

        let mut name = String::new();
        let mut description = String::new();

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("name:") {
                name = stripped.trim().trim_matches('"').to_string();
            } else if let Some(stripped) = line.strip_prefix("description:") {
                description = stripped.trim().trim_matches('"').to_string();
            }
        }

        if name.is_empty() {
            name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
        }

        Ok(Skill {
            name,
            description,
            body: body.trim().to_string(),
            path: path.parent().unwrap().to_path_buf(),
        })
    }

    pub fn generate_system_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("\n\n# Available Skills\n\n");
        for skill in &self.skills {
            prompt.push_str(&format!("## Skill: {}\n", skill.name));
            prompt.push_str(&format!("Description: {}\n", skill.description));
            prompt.push_str(&format!("Path: {}\n\n", skill.path.to_string_lossy()));
            prompt.push_str(&format!("### Instructions for {}\n", skill.name));
            prompt.push_str(&skill.body);
            prompt.push_str("\n\n---\n\n");
        }
        prompt
    }
}
