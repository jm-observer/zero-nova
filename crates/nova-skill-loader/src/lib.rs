use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SKILL_MARKDOWN_FILE: &str = "SKILL.md";
const SKILL_TOML_FILE: &str = "skill.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LoadedToolPolicy {
    InheritAll,
    AllowList(Vec<String>),
    AllowListWithDeferred(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedSkillPackage {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub instructions: String,
    pub tool_policy: LoadedToolPolicy,
    pub sticky: bool,
    pub aliases: Vec<String>,
    pub examples: Vec<String>,
    pub source_path: PathBuf,
    pub compat_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedCompatSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
    pub compat_mode: bool,
}

#[derive(Debug, Clone)]
pub enum LoadedSkill {
    Package(LoadedSkillPackage),
    Compat {
        skill: LoadedCompatSkill,
        package: LoadedSkillPackage,
    },
}

pub fn load_skills_from_dir(dir: impl AsRef<Path>) -> Result<Vec<LoadedSkill>> {
    let dir = dir.as_ref();
    if !dir.exists() || !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    scan_dir_recursive(dir, &mut skills)?;
    Ok(skills)
}

pub async fn load_skills_from_dir_async(dir: impl AsRef<Path>) -> Result<Vec<LoadedSkill>> {
    let dir = dir.as_ref();
    if !dir.exists() || !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(current_dir) = dirs.pop() {
        let mut entries = tokio::fs::read_dir(&current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if is_skill_dir(&path) {
                    if let Some(skill) = load_single_skill_async(&path).await? {
                        skills.push(skill);
                    }
                }
                dirs.push(path);
            }
        }
    }
    Ok(skills)
}

pub fn load_single_skill(path: impl AsRef<Path>) -> Result<Option<LoadedSkill>> {
    let path = path.as_ref();
    let skill_toml_path = path.join(SKILL_TOML_FILE);
    if skill_toml_path.exists() {
        return parse_skill_toml(&skill_toml_path).map(|package| Some(LoadedSkill::Package(package)));
    }

    let skill_md = path.join(SKILL_MARKDOWN_FILE);
    if skill_md.exists() {
        let skill = parse_skill_file(&skill_md)?;
        let package = to_skill_package(&skill);
        return Ok(Some(LoadedSkill::Compat { skill, package }));
    }

    Ok(None)
}

pub async fn load_single_skill_async(path: impl AsRef<Path>) -> Result<Option<LoadedSkill>> {
    let path = path.as_ref();
    let skill_toml_path = path.join(SKILL_TOML_FILE);
    if skill_toml_path.exists() {
        return parse_skill_toml_async(&skill_toml_path)
            .await
            .map(|package| Some(LoadedSkill::Package(package)));
    }

    let skill_md = path.join(SKILL_MARKDOWN_FILE);
    if skill_md.exists() {
        let skill = parse_skill_file_async(&skill_md).await?;
        let package = to_skill_package(&skill);
        return Ok(Some(LoadedSkill::Compat { skill, package }));
    }

    Ok(None)
}

fn scan_dir_recursive(dir: &Path, skills: &mut Vec<LoadedSkill>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if is_skill_dir(&path) {
                if let Some(skill) = load_single_skill(&path)? {
                    skills.push(skill);
                }
            }
            scan_dir_recursive(&path, skills)?;
        }
    }
    Ok(())
}

fn is_skill_dir(path: &Path) -> bool {
    path.join(SKILL_MARKDOWN_FILE).exists() || path.join(SKILL_TOML_FILE).exists()
}

fn parse_skill_file(path: &Path) -> Result<LoadedCompatSkill> {
    let content = std::fs::read_to_string(path)?;
    parse_skill_content(path, content)
}

async fn parse_skill_file_async(path: &Path) -> Result<LoadedCompatSkill> {
    let content = tokio::fs::read_to_string(path).await?;
    parse_skill_content(path, content)
}

fn parse_skill_content(path: &Path, content: String) -> Result<LoadedCompatSkill> {
    let Some(skill_dir) = path.parent() else {
        return Ok(LoadedCompatSkill {
            name: "unknown".to_string(),
            description: String::new(),
            body: content,
            path: PathBuf::new(),
            compat_mode: true,
        });
    };

    let parts: Vec<&str> = content.split("---").collect();
    if parts.len() < 3 {
        return Ok(LoadedCompatSkill {
            name: fallback_skill_name(skill_dir),
            description: String::new(),
            body: content,
            path: skill_dir.to_path_buf(),
            compat_mode: true,
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

    let fallback_name = fallback_skill_name(skill_dir);
    let compat_mode = name.is_empty();
    Ok(LoadedCompatSkill {
        name: if name.is_empty() { fallback_name } else { name },
        description,
        body: body.trim().to_string(),
        path: skill_dir.to_path_buf(),
        compat_mode,
    })
}

fn to_skill_package(skill: &LoadedCompatSkill) -> LoadedSkillPackage {
    let slug = skill
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&skill.name)
        .to_string();

    LoadedSkillPackage {
        id: slug.clone(),
        slug,
        display_name: skill.name.clone(),
        description: skill.description.clone(),
        instructions: skill.body.clone(),
        tool_policy: LoadedToolPolicy::InheritAll,
        sticky: false,
        aliases: vec![],
        examples: vec![],
        source_path: skill.path.clone(),
        compat_mode: true,
    }
}

fn parse_skill_toml(path: &Path) -> Result<LoadedSkillPackage> {
    let content = std::fs::read_to_string(path)?;
    parse_skill_toml_content(path, content)
}

async fn parse_skill_toml_async(path: &Path) -> Result<LoadedSkillPackage> {
    let content = tokio::fs::read_to_string(path).await?;
    parse_skill_toml_content(path, content)
}

fn parse_skill_toml_content(path: &Path, content: String) -> Result<LoadedSkillPackage> {
    let toml: toml::Value = toml::from_str(&content)?;
    let slug = toml_string(&toml, "slug")
        .or_else(|| toml_string(&toml, "id"))
        .unwrap_or_else(|| fallback_skill_name(path));

    Ok(LoadedSkillPackage {
        id: slug.clone(),
        slug: slug.clone(),
        display_name: toml_string(&toml, "display_name").unwrap_or_else(|| slug.clone()),
        description: toml_string(&toml, "description").unwrap_or_default(),
        instructions: toml_string(&toml, "instructions").unwrap_or_default(),
        tool_policy: parse_tool_policy(&toml),
        sticky: toml.get("sticky").and_then(|v| v.as_bool()).unwrap_or(false),
        aliases: toml_array_strings(&toml, "aliases"),
        examples: toml_array_strings(&toml, "examples"),
        source_path: path.to_path_buf(),
        compat_mode: false,
    })
}

fn parse_tool_policy(toml: &toml::Value) -> LoadedToolPolicy {
    match toml
        .get("tool_policy")
        .and_then(|v| v.as_str())
        .unwrap_or("inherit_all")
    {
        "allow_list" => LoadedToolPolicy::AllowList(tool_allow_list(toml)),
        "allow_list_with_deferred" => LoadedToolPolicy::AllowListWithDeferred(tool_allow_list(toml)),
        _ => LoadedToolPolicy::InheritAll,
    }
}

fn tool_allow_list(toml: &toml::Value) -> Vec<String> {
    toml.get("tool_policy")
        .and_then(|tool_policy| tool_policy.get("allow_list"))
        .and_then(|allow_list| allow_list.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn toml_string(toml: &toml::Value, key: &str) -> Option<String> {
    toml.get(key).and_then(|value| value.as_str()).map(str::to_string)
}

fn toml_array_strings(toml: &toml::Value, key: &str) -> Vec<String> {
    toml.get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn fallback_skill_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}
