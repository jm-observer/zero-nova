use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub top_p: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,                // "OpenClaw", "oc", "open-claw"
    pub system_prompt_template: String,      // 该 agent 的 system prompt 模板
    pub tool_whitelist: Option<Vec<String>>, // None = 全部工具
    pub model_config: Option<ModelConfig>,   // None = 使用默认
}

#[derive(Debug, Clone)]
pub struct AgentRegistry {
    agents: HashMap<String, AgentDescriptor>,
    primary_agent: String,
}

impl AgentRegistry {
    pub fn new(primary: AgentDescriptor) -> Self {
        let mut agents = HashMap::new();
        let primary_id = primary.id.clone();
        agents.insert(primary_id.clone(), primary);
        Self {
            agents,
            primary_agent: primary_id,
        }
    }

    pub fn register(&mut self, agent: AgentDescriptor) {
        self.agents.insert(agent.id.clone(), agent);
    }

    pub fn get(&self, id: &str) -> Option<&AgentDescriptor> {
        self.agents.get(id)
    }

    pub fn list(&self) -> Vec<&AgentDescriptor> {
        self.agents.values().collect()
    }

    pub fn primary_id(&self) -> &str {
        &self.primary_agent
    }

    /// 精确匹配 agent id / display_name / aliases
    /// 初版只做精确匹配（大小写不敏感），不支持指代（"之前那个"）
    pub fn resolve_addressing(&self, text: &str) -> Option<String> {
        let lower = text.to_lowercase();

        for (id, desc) in &self.agents {
            // 匹配模式："让 XX 处理" / "XX 在不在" / "XX，帮我看看" / "@XX"
            let names =
                std::iter::once(desc.display_name.to_lowercase()).chain(desc.aliases.iter().map(|a| a.to_lowercase()));

            for name in names {
                if lower.contains(&name) {
                    return Some(id.clone());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_registry() -> AgentRegistry {
        let primary = AgentDescriptor {
            id: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            description: "Default agent".to_string(),
            aliases: vec!["oc".to_string(), "open-claw".to_string()],
            system_prompt_template: "You are OpenClaw".to_string(),
            tool_whitelist: None,
            model_config: None,
        };
        AgentRegistry::new(primary)
    }

    #[test]
    fn test_resolve_by_display_name() {
        let registry = setup_registry();
        assert_eq!(
            registry.resolve_addressing("让 OpenClaw 处理一下"),
            Some("openclaw".to_string())
        );
    }

    #[test]
    fn test_resolve_by_alias() {
        let registry = setup_registry();
        assert_eq!(
            registry.resolve_addressing("oc, 帮我看看"),
            Some("openclaw".to_string())
        );
    }

    #[test]
    fn test_resolve_case_insensitive() {
        let registry = setup_registry();
        assert_eq!(
            registry.resolve_addressing("openclaw 在吗"),
            Some("openclaw".to_string())
        );
    }

    #[test]
    fn test_resolve_no_match() {
        let registry = setup_registry();
        assert_eq!(registry.resolve_addressing("让某个不存在的agent处理"), None);
    }
}
