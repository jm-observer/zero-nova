//! 配置校验逻辑。
//!
//! 此模块包含 `AppConfig` 的校验方法 `validate()` 及相关辅助函数。

use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::PathBuf;

use super::models::*;

impl AppConfig {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            ..Self::default()
        }
    }

    pub fn find_agent(&self, agent_id: &str) -> Result<&AgentSpec> {
        self.gateway
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not found", agent_id))
    }

    pub fn primary_agent(&self) -> Result<&AgentSpec> {
        self.gateway
            .agents
            .first()
            .ok_or_else(|| anyhow::anyhow!("gateway.agents cannot be empty"))
    }

    pub fn selected_agent(&self, agent_id: Option<&str>) -> Result<&AgentSpec> {
        match agent_id {
            Some(agent_id) => self.find_agent(agent_id),
            None => self.primary_agent(),
        }
    }

    pub fn resolve_agent_binding(&self, agent: &AgentSpec) -> Result<ResolvedAgentBinding> {
        let binding = self.resolve_named_binding(agent.provider.as_str(), &agent.llm)?;
        if binding.provider_id != agent.provider {
            bail!(
                "agent '{}' llm '{}' belongs to provider '{}', expected '{}'",
                agent.id,
                agent.llm,
                binding.provider_id,
                agent.provider
            );
        }
        Ok(binding)
    }

    pub fn resolve_agent_binding_by_id(&self, agent_id: &str) -> Result<ResolvedAgentBinding> {
        let agent = self.find_agent(agent_id)?;
        self.resolve_agent_binding(agent)
    }

    pub fn resolve_model_override(
        &self,
        base_binding: &ResolvedAgentBinding,
        provider_id: &str,
        model_or_llm: &str,
    ) -> Result<ResolvedAgentBinding> {
        let provider_id = provider_id.trim();
        let model_or_llm = model_or_llm.trim();
        if provider_id.is_empty() {
            bail!("provider override cannot be empty");
        }
        if model_or_llm.is_empty() {
            bail!("model override cannot be empty");
        }

        if let Some(llm) = self.llms.get(model_or_llm) {
            if llm.provider != provider_id {
                bail!(
                    "override model '{}' belongs to provider '{}', expected '{}'",
                    model_or_llm,
                    llm.provider,
                    provider_id
                );
            }
            return self.resolve_named_binding(provider_id, model_or_llm);
        }

        let provider = self
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider override '{}'", provider_id))?;
        let mut model_config = base_binding.model_config.clone();
        model_config.provider = Some(provider_id.to_string());
        model_config.model = model_or_llm.to_string();
        Ok(ResolvedAgentBinding {
            provider_id: provider_id.to_string(),
            provider,
            llm_id: None,
            model_config,
        })
    }

    fn resolve_named_binding(&self, provider_id: &str, llm_id: &str) -> Result<ResolvedAgentBinding> {
        let provider = self
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider '{}'", provider_id))?;
        let llm = self
            .llms
            .get(llm_id)
            .ok_or_else(|| anyhow::anyhow!("unknown llm '{}'", llm_id))?;
        if llm.provider != provider_id {
            bail!(
                "llm '{}' belongs to provider '{}', expected '{}'",
                llm_id,
                llm.provider,
                provider_id
            );
        }
        let mut model_config = llm.model_config.clone();
        model_config.provider = Some(provider_id.to_string());
        Ok(ResolvedAgentBinding {
            provider_id: provider_id.to_string(),
            provider,
            llm_id: Some(llm_id.to_string()),
            model_config,
        })
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.config_dir
            .join(self.tool.skills_dir.as_deref().unwrap_or("skills"))
    }

    pub fn data_dir_path(&self) -> PathBuf {
        self.config_dir.join("data")
    }

    pub fn prompts_dir(&self) -> PathBuf {
        self.config_dir
            .join(self.tool.prompts_dir.as_deref().unwrap_or("prompts"))
    }

    pub fn project_context_file(&self) -> Option<PathBuf> {
        self.tool.project_context_file.as_deref().map(|path| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                self.config_dir.join(path)
            }
        })
    }

    pub fn config_path(&self) -> PathBuf {
        match &self.config_path {
            Some(path) => {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    self.config_dir.join(path)
                }
            }
            None => self.config_dir.join("config.toml"),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            bail!("providers cannot be empty");
        }
        if self.llms.is_empty() {
            bail!("llms cannot be empty");
        }

        for (llm_id, llm) in &self.llms {
            if llm.provider.trim().is_empty() {
                bail!("llm '{}' provider cannot be empty", llm_id);
            }
            if !self.providers.contains_key(llm.provider.as_str()) {
                bail!("llm '{}' references unknown provider '{}'", llm_id, llm.provider);
            }
        }

        if self.gateway.agents.is_empty() {
            bail!("gateway.agents cannot be empty");
        }

        let mut ids = HashSet::new();
        for agent in &self.gateway.agents {
            if !ids.insert(agent.id.clone()) {
                bail!("duplicate agent id found: {}", agent.id);
            }
            if agent.provider.trim().is_empty() {
                bail!("agent '{}' provider cannot be empty", agent.id);
            }
            if !self.providers.contains_key(agent.provider.as_str()) {
                bail!("agent '{}' references unknown provider '{}'", agent.id, agent.provider);
            }
            if agent.llm.trim().is_empty() {
                bail!("agent '{}' llm cannot be empty", agent.id);
            }
            let llm = self
                .llms
                .get(&agent.llm)
                .ok_or_else(|| anyhow::anyhow!("agent '{}' references unknown llm '{}'", agent.id, agent.llm))?;
            if llm.provider != agent.provider {
                bail!(
                    "agent '{}' llm '{}' belongs to provider '{}', expected '{}'",
                    agent.id,
                    agent.llm,
                    llm.provider,
                    agent.provider
                );
            }
            if agent.prompt_file.is_some() && agent.prompt_inline.is_some() {
                bail!("agent '{}' cannot set both prompt_file and prompt_inline", agent.id);
            }
        }

        if !matches!(
            self.gateway.skill_history_strategy.as_str(),
            "global" | "per_skill" | "segments"
        ) {
            bail!(
                "gateway.skill_history_strategy must be one of: global, per_skill, segments; got '{}'",
                self.gateway.skill_history_strategy
            );
        }

        if !matches!(
            self.gateway.loop_guard.duplicate_read_mode.as_str(),
            "warn_then_reject" | "warn_only"
        ) {
            bail!(
                "gateway.loop_guard.duplicate_read_mode must be one of: warn_then_reject, warn_only; got '{}'",
                self.gateway.loop_guard.duplicate_read_mode
            );
        }

        if !(0.0..1.0).contains(&self.gateway.loop_guard.iteration_trim_ratio) {
            bail!(
                "gateway.loop_guard.iteration_trim_ratio must be in (0, 1), got {}",
                self.gateway.loop_guard.iteration_trim_ratio
            );
        }

        if self.gateway.tool_result_compaction.max_chars == 0 {
            bail!("gateway.tool_result_compaction.max_chars must be greater than 0");
        }
        if self.gateway.tool_result_compaction.head_chars + self.gateway.tool_result_compaction.tail_chars
            >= self.gateway.tool_result_compaction.max_chars
        {
            bail!("gateway.tool_result_compaction.head_chars + tail_chars must be less than max_chars");
        }

        for (i, file) in self.developer_prompt_files.iter().enumerate() {
            if file.trim().is_empty() {
                bail!("developer_prompt_files[{}] cannot be empty", i);
            }
        }

        if !matches!(
            self.prompt_compaction.project_instruction_profile.as_str(),
            "auto" | "analysis" | "code" | "design" | "review" | "full"
        ) {
            bail!("prompt_compaction.project_instruction_profile is invalid");
        }
        if !matches!(
            self.prompt_compaction.skill_injection.as_str(),
            "catalog" | "active_full" | "full"
        ) {
            bail!("prompt_compaction.skill_injection is invalid");
        }
        if !matches!(self.prompt_compaction.tool_guidance.as_str(), "compact" | "full") {
            bail!("prompt_compaction.tool_guidance is invalid");
        }

        if !matches!(self.gateway.max_tokens_field.as_str(), "completion" | "legacy" | "both") {
            bail!("gateway.max_tokens_field is invalid");
        }

        if self.search.backend.as_deref() == Some("tavily")
            && self
                .search
                .tavily_api_key
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            bail!("search.backend is tavily but tavily_api_key is missing (or TAVILY_API_KEY is not set)");
        }

        Ok(())
    }
}
