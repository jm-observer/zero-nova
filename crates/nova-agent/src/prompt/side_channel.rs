use crate::skill::SkillRegistry;

/// 侧信道注入配置。
#[derive(Debug, Clone)]
pub struct SideChannelConfig {
    /// 是否启用侧信道
    pub enabled: bool,
    /// 注入 skill 列表的间隔（每 N 次 tool result 注入一次）
    pub skill_reminder_interval: usize,
    /// 是否注入当前日期
    pub inject_date: bool,
    /// 自定义注入内容
    pub custom_reminders: Vec<String>,
}

impl Default for SideChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skill_reminder_interval: 5,
            inject_date: true,
            custom_reminders: vec![],
        }
    }
}

/// 侧信道注入器。
pub struct SideChannelInjector {
    config: SideChannelConfig,
    tool_result_counter: std::sync::atomic::AtomicUsize,
}

impl SideChannelInjector {
    pub fn new(config: SideChannelConfig) -> Self {
        Self {
            config,
            tool_result_counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn generate_injection(&self, skills: &SkillRegistry) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let count = self
            .tool_result_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if !count.is_multiple_of(self.config.skill_reminder_interval) {
            return None;
        }

        let mut parts = Vec::new();

        if !skills.packages.is_empty() {
            let skill_list: Vec<String> = skills
                .packages
                .iter()
                .map(|p| format!("- {}: {}", p.slug, p.description))
                .collect();
            parts.push(format!(
                "<system-reminder>\nAvailable skills:\n{}\n\nUse /skill-<name> to activate.\n</system-reminder>",
                skill_list.join("\n")
            ));
        }

        if self.config.inject_date {
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            parts.push(format!("<system-reminder>\nCurrent date: {}\n</system-reminder>", date));
        }

        for reminder in &self.config.custom_reminders {
            parts.push(format!("<system-reminder>\n{}\n</system-reminder>", reminder));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    pub fn inject_into_tool_result(&self, tool_output: &str, skills: &SkillRegistry) -> String {
        match self.generate_injection(skills) {
            Some(injection) => format!("{}\n\n{}", tool_output, injection),
            None => tool_output.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{SkillPackage, ToolPolicy};

    #[test]
    fn side_channel_disabled_returns_original() {
        let injector = SideChannelInjector::new(SideChannelConfig {
            enabled: false,
            skill_reminder_interval: 1,
            inject_date: false,
            custom_reminders: vec![],
        });
        let registry = SkillRegistry::new();

        assert_eq!(
            injector.inject_into_tool_result("tool output", &registry),
            "tool output"
        );
    }

    #[test]
    fn side_channel_injects_skill_and_custom_reminder() {
        let injector = SideChannelInjector::new(SideChannelConfig {
            enabled: true,
            skill_reminder_interval: 1,
            inject_date: false,
            custom_reminders: vec!["Remember policy".to_string()],
        });
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First".to_string(),
            instructions: "Do work".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: std::path::PathBuf::from("skill-1"),
            compat_mode: false,
        });

        let result = injector.inject_into_tool_result("tool output", &registry);
        assert!(result.contains("tool output"));
        assert!(result.contains("Available skills:"));
        assert!(result.contains("Remember policy"));
    }
}
