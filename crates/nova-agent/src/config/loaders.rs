//! 配置加载与迁移逻辑。
//!
//! 此模块包含所有 `Raw*` 结构体定义、迁移方法、文件加载函数和
//! 环境变量覆盖逻辑，与模型层和校验层职责分离。

use crate::agent_catalog::AgentModelOverride;
use serde::de::{self, IgnoredAny};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::models::*;

pub type Result<T> = anyhow::Result<T>;

/// 从文件加载配置，执行迁移、环境变量覆盖和校验。
pub fn load_from_file(path: PathBuf, config_dir: PathBuf) -> Result<AppConfig> {
    let content = fs::read_to_string(path)?;
    let raw_config: RawAppConfig = toml::from_str(&content)?;
    let (mut config, warnings) = raw_config.migrate(config_dir);
    config.apply_env_overrides();
    config.validate()?;
    for warning in &warnings {
        log::warn!("{}", warning);
    }
    Ok(config)
}

#[derive(Debug, Deserialize, Default)]
pub struct RawAppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    llms: HashMap<String, RawRegisteredLlmConfig>,
    #[serde(rename = "defaults", default, deserialize_with = "reject_removed_defaults")]
    _removed_defaults: Option<IgnoredAny>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    tool: RawToolConfig,
    #[serde(default)]
    gateway: RawGatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub developer_prompt_files: Vec<String>,
    #[serde(default)]
    pub prompt_compaction: PromptCompactionConfig,
    #[serde(default)]
    pub outbound_context_headers: OutboundContextHeaderConfig,
}

fn reject_removed_defaults<'de, D>(deserializer: D) -> std::result::Result<Option<IgnoredAny>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<IgnoredAny>::deserialize(deserializer)?;
    if value.is_some() {
        return Err(de::Error::custom(
            "[defaults] has been removed; bind provider/llm explicitly on each [[gateway.agents]] entry",
        ));
    }
    Ok(None)
}

#[derive(Debug, Deserialize, Default, Clone)]
struct RawToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    pub skills_dir: Option<String>,
    #[serde(default)]
    pub prompts_dir: Option<String>,
    #[serde(default)]
    pub project_context_file: Option<String>,
    #[serde(default)]
    pub default_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawRegisteredLlmConfig {
    provider: String,
    #[serde(flatten)]
    model_config: RawModelConfig,
}

#[derive(Debug, Deserialize, Default)]
struct RawModelConfig {
    model: String,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: f32,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    thinking_budget: Option<u32>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    extra_body: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct RawGatewayConfig {
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    #[serde(default)]
    tool_timeout_secs: Option<u64>,
    #[serde(default = "default_subagent_timeout")]
    subagent_timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    max_tokens: usize,
    #[serde(default = "default_max_tokens_field")]
    max_tokens_field: String,
    #[serde(default)]
    agents: Vec<RawAgentSpec>,
    #[serde(default)]
    skill_routing_enabled: bool,
    #[serde(default = "default_skill_history_strategy")]
    skill_history_strategy: String,
    #[serde(default)]
    trimmer: RawTrimmerConfigToml,
    #[serde(default)]
    side_channel: SideChannelConfigToml,
    #[serde(default)]
    loop_guard: LoopGuardConfigToml,
    #[serde(default)]
    prompt_diagnostics: PromptDiagnosticsConfigToml,
    #[serde(default)]
    tool_result_compaction: ToolResultCompactionConfigToml,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct RawAgentSpec {
    id: String,
    display_name: String,
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    llm: String,
    #[serde(default)]
    prompt_file: Option<String>,
    #[serde(default)]
    prompt_inline: Option<String>,
    #[serde(default)]
    system_prompt_template: Option<String>,
    #[serde(default)]
    tool_whitelist: Option<Vec<String>>,
    #[serde(default)]
    model_config: Option<AgentModelOverride>,
    #[serde(default)]
    pub enable_project_developer_prompt: bool,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct RawTrimmerConfigToml {
    #[serde(default = "default_trimmer_enabled")]
    enabled: bool,
    #[serde(default = "default_context_window")]
    context_window: usize,
    #[serde(default = "default_output_reserve")]
    output_reserve: usize,
    #[serde(default = "default_min_recent")]
    min_recent_messages: usize,
    #[serde(default)]
    max_history_tokens: Option<usize>,
    #[serde(default)]
    preserve_recent: Option<usize>,
    #[serde(default)]
    preserve_tool_pairs: Option<bool>,
}

impl RawAppConfig {
    pub fn migrate(self, config_dir: PathBuf) -> (AppConfig, Vec<String>) {
        let mut warnings = Vec::new();

        let llms: HashMap<String, RegisteredLlmConfig> = self
            .llms
            .into_iter()
            .map(|(llm_id, raw_llm)| {
                let mut model_config = crate::provider::ModelConfig {
                    provider: Some(raw_llm.provider.clone()),
                    model: raw_llm.model_config.model,
                    max_tokens: raw_llm.model_config.max_tokens.unwrap_or(LlmConfig::default().model_config.max_tokens),
                    temperature: Some(raw_llm.model_config.temperature),
                    top_p: raw_llm.model_config.top_p,
                    thinking_budget: raw_llm.model_config.thinking_budget,
                    reasoning_effort: raw_llm.model_config.reasoning_effort,
                    max_tokens_field: self.gateway.max_tokens_field.clone(),
                    extra_body: raw_llm.model_config.extra_body,
                };
                if model_config.thinking_budget.is_some() && model_config.reasoning_effort.is_some() {
                    model_config.reasoning_effort = None;
                    warnings.push(format!(
                        "Both llms.{}.thinking_budget and llms.{}.reasoning_effort are set; preferring thinking_budget and ignoring reasoning_effort.",
                        llm_id, llm_id
                    ));
                }
                (
                    llm_id,
                    RegisteredLlmConfig {
                        provider: raw_llm.provider,
                        model_config,
                    },
                )
            })
            .collect();

        let mut migrated_agents = Vec::with_capacity(self.gateway.agents.len());
        for mut agent in self.gateway.agents {
            if agent.prompt_file.is_none() && agent.prompt_inline.is_none() {
                if let Some(legacy_prompt) = agent.system_prompt_template.take() {
                    if looks_like_prompt_file(&legacy_prompt) {
                        agent.prompt_file = Some(legacy_prompt);
                        warnings.push(format!(
                            "Agent '{}' uses deprecated system_prompt_template; migrated to prompt_file.",
                            agent.id,
                        ));
                    } else {
                        agent.prompt_inline = Some(legacy_prompt);
                        warnings.push(format!(
                            "Agent '{}' uses deprecated system_prompt_template; migrated to prompt_inline.",
                            agent.id,
                        ));
                    }
                }
            }

            let model_config = if let Some(model_config) = agent.model_config.take() {
                model_config
            } else if let Some(llm) = llms.get(&agent.llm) {
                AgentModelOverride {
                    model: llm.model_config.model.clone(),
                    temperature: llm.model_config.temperature.unwrap_or(0.0),
                    max_tokens: Some(llm.model_config.max_tokens),
                    top_p: llm.model_config.top_p.unwrap_or(1.0),
                }
            } else {
                AgentModelOverride {
                    model: LlmConfig::default().model_config.model,
                    temperature: LlmConfig::default().model_config.temperature.unwrap_or(0.0),
                    max_tokens: Some(LlmConfig::default().model_config.max_tokens),
                    top_p: LlmConfig::default().model_config.top_p.unwrap_or(1.0),
                }
            };

            migrated_agents.push(AgentSpec {
                id: agent.id,
                display_name: agent.display_name,
                description: agent.description,
                aliases: agent.aliases,
                provider: agent.provider,
                llm: agent.llm,
                prompt_file: agent.prompt_file,
                prompt_inline: agent.prompt_inline,
                system_prompt_template: None,
                tool_whitelist: agent.tool_whitelist,
                model_config,
                enable_project_developer_prompt: agent.enable_project_developer_prompt,
            });
        }

        let mut trimmer = super::models::TrimmerConfigToml {
            enabled: self.gateway.trimmer.enabled,
            context_window: self.gateway.trimmer.context_window,
            output_reserve: self.gateway.trimmer.output_reserve,
            min_recent_messages: self.gateway.trimmer.min_recent_messages,
        };
        if let Some(max_history_tokens) = self.gateway.trimmer.max_history_tokens {
            trimmer.enabled = true;
            trimmer.context_window = max_history_tokens + trimmer.output_reserve;
            warnings.push(
                "Detected deprecated gateway.trimmer.max_history_tokens; migrated to context_window + output_reserve."
                    .to_string(),
            );
        }
        if let Some(preserve_recent) = self.gateway.trimmer.preserve_recent {
            trimmer.min_recent_messages = preserve_recent;
            warnings.push(
                "Detected deprecated gateway.trimmer.preserve_recent; migrated to gateway.trimmer.min_recent_messages."
                    .to_string(),
            );
        }
        if self.gateway.trimmer.preserve_tool_pairs.is_some() {
            warnings.push(
                "gateway.trimmer.preserve_tool_pairs is deprecated and currently not implemented; this field is ignored."
                    .to_string(),
            );
        }

        (
            AppConfig {
                providers: self.providers,
                llms,
                search: self.search,
                tool: ToolConfig {
                    bash: self.tool.bash,
                    skills_dir: self.tool.skills_dir,
                    prompts_dir: self.tool.prompts_dir,
                    project_context_file: self.tool.project_context_file,
                    default_policy: self.tool.default_policy,
                },
                gateway: GatewayConfig {
                    host: self.gateway.host,
                    port: self.gateway.port,
                    max_iterations: self.gateway.max_iterations,
                    tool_timeout_secs: self.gateway.tool_timeout_secs,
                    subagent_timeout_secs: self.gateway.subagent_timeout_secs,
                    max_tokens: self.gateway.max_tokens,
                    max_tokens_field: self.gateway.max_tokens_field,
                    agents: migrated_agents,
                    skill_routing_enabled: self.gateway.skill_routing_enabled,
                    skill_history_strategy: self.gateway.skill_history_strategy,
                    trimmer,
                    side_channel: self.gateway.side_channel,
                    loop_guard: self.gateway.loop_guard,
                    prompt_diagnostics: self.gateway.prompt_diagnostics,
                    tool_result_compaction: self.gateway.tool_result_compaction,
                },
                voice: self.voice,
                config_dir,
                config_path: self.config_path,
                developer_prompt_files: self.developer_prompt_files,
                prompt_compaction: self.prompt_compaction,
                outbound_context_headers: self.outbound_context_headers,
            },
            warnings,
        )
    }
}

pub fn looks_like_prompt_file(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.ends_with(".md") || trimmed.ends_with(".txt") || trimmed.contains('/') || trimmed.contains('\\')
}

impl AppConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P, config_dir: PathBuf) -> Result<Self> {
        load_from_file(path.as_ref().to_path_buf(), config_dir)
    }

    pub fn apply_env_overrides(&mut self) {
        if let Ok(tavily_api_key) = env::var("TAVILY_API_KEY") {
            if !tavily_api_key.is_empty() {
                self.search.tavily_api_key = Some(tavily_api_key);
            }
        }
    }
}
