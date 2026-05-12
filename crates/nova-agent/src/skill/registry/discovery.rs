use super::SkillRegistry;
use anyhow::Result;
use std::path::Path;

impl SkillRegistry {
    /// 递归扫描 skill 根目录，加载所有 SKILL.md 和 skill.toml。
    pub fn load_from_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<()> {
        let dir = dir.as_ref();
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }
        Self::scan_dir_recursive(dir, self)?;
        Ok(())
    }

    /// 异步加载技能目录，适用于 async 上下文。
    pub async fn load_from_dir_async<P: AsRef<Path>>(&mut self, dir: P) -> Result<()> {
        let dir = dir.as_ref();
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }
        Self::scan_dir_recursive_async(dir, self).await?;
        Ok(())
    }

    /// 递归扫描目录。
    fn scan_dir_recursive(dir: &Path, registry: &mut SkillRegistry) -> Result<()> {
        let entries = read_dir_runtime_aware(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // 尝试加载子目录
                let skill_md = path.join("SKILL.md");
                let skill_toml = path.join("skill.toml");
                if skill_md.exists() || skill_toml.exists() {
                    registry.load_single_skill(&path)?;
                }
                // 继续递归子目录
                Self::scan_dir_recursive(&path, registry)?;
            } else {
                // 直接加载文件 - 递归扫描时直接使用 load_single_skill
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    registry.load_single_skill(&path)?;
                }
            }
        }
        Ok(())
    }

    /// 异步递归扫描目录（使用显式栈避免 async 递归）。
    async fn scan_dir_recursive_async(dir: &Path, registry: &mut SkillRegistry) -> Result<()> {
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(current_dir) = dirs.pop() {
            let mut entries = tokio::fs::read_dir(&current_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    let skill_md = path.join("SKILL.md");
                    let skill_toml = path.join("skill.toml");
                    if skill_md.exists() || skill_toml.exists() {
                        registry.load_single_skill_async(&path).await?;
                    }
                    dirs.push(path);
                } else {
                    let skill_md = path.join("SKILL.md");
                    if skill_md.exists() {
                        registry.load_single_skill_async(&path).await?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// 启动期/测试同步目录遍历辅助。
/// 运行时热路径应优先使用 async discovery API（load_from_dir_async）。
fn read_dir_runtime_aware(path: &Path) -> std::io::Result<std::fs::ReadDir> {
    std::fs::read_dir(path)
}
