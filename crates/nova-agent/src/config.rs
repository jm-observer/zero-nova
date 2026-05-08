use crate::agent_catalog::ModelConfig as AgentModelConfig;
use crate::provider::ModelConfig;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_BINDING_PROVIDER: &str = "default";
const DEFAULT_BINDING_LLM: &str = "default";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OriginAppConfig {
    #[serde(default = "default_provider_registry")]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default = "default_llm_registry")]
    pub llms: HashMap<String, RegisteredLlmConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: ToolConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    /// Path to the configuration file relative to config_dir. When None, defaults to `config.toml`.
    #[serde(default)]
    pub config_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default = "default_provider_registry")]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default = "default_llm_registry")]
    pub llms: HashMap<String, RegisteredLlmConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: ToolConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(alias = "workspace")]
    pub config_dir: PathBuf,
    /// Path to the configuration file relative to config_dir. When None, defaults to `config.toml`.
    pub config_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceConfig {
    #[serde(default = "default_voice_enabled")]
    pub enabled: bool,
    #[serde(default = "default_stt_model")]
    pub stt_model: String,
    #[serde(default = "default_tts_model")]
    pub tts_model: String,
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    #[serde(default = "default_stt_timeout_ms")]
    pub stt_timeout_ms: u64,
    #[serde(default = "default_tts_timeout_ms")]
    pub tts_timeout_ms: u64,
    #[serde(default = "default_voice_max_input_bytes")]
    pub max_input_bytes: usize,
    #[serde(default)]
    pub auto_play: bool,
    #[serde(default = "default_voice_provider")]
    pub provider: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    #[serde(flatten)]
    pub model_config: ModelConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RegisteredLlmConfig {
    pub provider: String,
    #[serde(flatten)]
    pub model_config: ModelConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedAgentBinding {
    pub provider_id: String,
    pub provider: ProviderConfig,
    pub llm_id: Option<String>,
    pub model_config: ModelConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DefaultBindingConfig {
    #[serde(default = "default_provider_binding_id")]
    pub provider: String,
    #[serde(default = "default_llm_binding_id")]
    pub llm: String,
}

fn default_base_url() -> String {
    "http://127.0.0.1:8082/v1".to_string()
}

fn default_provider_binding_id() -> String {
    DEFAULT_BINDING_PROVIDER.to_string()
}

fn default_llm_binding_id() -> String {
    DEFAULT_BINDING_LLM.to_string()
}

fn default_provider_registry() -> HashMap<String, ProviderConfig> {
    HashMap::from([(default_provider_binding_id(), ProviderConfig::default())])
}

fn default_llm_registry() -> HashMap<String, RegisteredLlmConfig> {
    let default_model = LlmConfig::default().model_config;
    HashMap::from([(
        default_llm_binding_id(),
        RegisteredLlmConfig {
            provider: default_provider_binding_id(),
            model_config: default_model,
        },
    )])
}

fn default_voice_enabled() -> bool {
    true
}

fn default_stt_model() -> String {
    "whisper-1".to_string()
}

fn default_tts_model() -> String {
    "tts-1".to_string()
}

fn default_tts_voice() -> String {
    "alloy".to_string()
}

fn default_stt_timeout_ms() -> u64 {
    30_000
}

fn default_tts_timeout_ms() -> u64 {
    30_000
}

fn default_voice_max_input_bytes() -> usize {
    5 * 1024 * 1024
}

fn default_voice_provider() -> String {
    "openai_compat".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_config: ModelConfig {
                provider: Some(default_provider_binding_id()),
                model: "gpt-oss-120b".to_string(),
                max_tokens: 8192,
                temperature: None,
                top_p: None,
                thinking_budget: None,
                reasoning_effort: None,
            },
        }
    }
}

impl Default for RegisteredLlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_binding_id(),
            model_config: LlmConfig::default().model_config,
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: default_base_url(),
        }
    }
}

impl Default for DefaultBindingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_binding_id(),
            llm: default_llm_binding_id(),
        }
    }
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: default_voice_enabled(),
            stt_model: default_stt_model(),
            tts_model: default_tts_model(),
            tts_voice: default_tts_voice(),
            stt_timeout_ms: default_stt_timeout_ms(),
            tts_timeout_ms: default_tts_timeout_ms(),
            max_input_bytes: default_voice_max_input_bytes(),
            auto_play: false,
            provider: default_voice_provider(),
        }
    }
}

impl Default for OriginAppConfig {
    fn default() -> Self {
        Self {
            providers: default_provider_registry(),
            llms: default_llm_registry(),
            search: SearchConfig::default(),
            tool: ToolConfig::default(),
            gateway: GatewayConfig::default(),
            voice: VoiceConfig::default(),
            config_path: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SearchConfig {
    pub backend: Option<String>,
    pub google_api_key: Option<String>,
    pub google_cx: Option<String>,
    pub google_endpoint: Option<String>,
    pub tavily_api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    pub skills_dir: Option<String>,
    /// Prompts directory for agent template files. When None, defaults to `{config_dir}/prompts`.
    #[serde(default)]
    pub prompts_dir: Option<String>,
    /// 项目上下文文件路径。为空时按默认候选文件自动查找。
    #[serde(default)]
    pub project_context_file: Option<String>,
    #[serde(default)]
    pub default_policy: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BashConfig {
    pub shell: Option<String>,
    pub sandbox: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentSpec {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub llm: String,
    /// 指向 prompts_dir 下的模板文件名
    #[serde(default)]
    pub prompt_file: Option<String>,
    /// 直接内联的 prompt 内容
    #[serde(default)]
    pub prompt_inline: Option<String>,
    #[serde(default)]
    pub system_prompt_template: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub model_config: AgentModelConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default = "default_subagent_timeout")]
    pub subagent_timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub agents: Vec<AgentSpec>,
    /// 是否启用自动 skill 路由 (Plan 1 新增)。
    #[serde(default)]
    pub skill_routing_enabled: bool,
    /// Skill 历史策略 ("global" | "per_skill" | "segments")。
    /// 对应 Plan 1/2/3 的演进阶段。
    #[serde(default = "default_skill_history_strategy")]
    pub skill_history_strategy: String,
    /// 是否启用新的 prepare_turn + run_turn_with_context 路径。
    #[serde(default)]
    pub use_turn_context: bool,
    /// 历史裁剪配置（Phase 3 新增）。
    #[serde(default)]
    pub trimmer: TrimmerConfigToml,
    /// 侧信道注入配置（Phase 3 新增）。
    #[serde(default)]
    pub side_channel: SideChannelConfigToml,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    18801
}
fn default_max_iterations() -> usize {
    30
}
fn default_subagent_timeout() -> u64 {
    300
}

fn default_max_tokens() -> usize {
    4096
}

fn default_skill_history_strategy() -> String {
    "global".to_string()
}
fn default_trimmer_enabled() -> bool {
    true
}
fn default_context_window() -> usize {
    128_000
}
fn default_output_reserve() -> usize {
    8_192
}
fn default_min_recent() -> usize {
    10
}
fn default_side_channel_enabled() -> bool {
    false
}
fn default_skill_reminder_interval() -> usize {
    5
}

/// 历史裁剪配置（TOML 序列化版本）。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrimmerConfigToml {
    /// 是否启用历史裁剪
    #[serde(default = "default_trimmer_enabled")]
    pub enabled: bool,
    /// 模型上下文窗口大小
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    /// 输出预留 token 数
    #[serde(default = "default_output_reserve")]
    pub output_reserve: usize,
    /// 最少保留的最近消息数
    #[serde(default = "default_min_recent")]
    pub min_recent_messages: usize,
}

/// 侧信道注入配置（TOML 序列化版本）。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SideChannelConfigToml {
    /// 是否启用侧信道
    #[serde(default = "default_side_channel_enabled")]
    pub enabled: bool,
    /// 注入 skill 列表的间隔
    #[serde(default = "default_skill_reminder_interval")]
    pub skill_reminder_interval: usize,
    /// 是否注入当前日期
    pub inject_date: Option<bool>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            max_iterations: default_max_iterations(),
            tool_timeout_secs: None,
            subagent_timeout_secs: default_subagent_timeout(),
            max_tokens: default_max_tokens(),
            agents: Vec::new(),
            skill_routing_enabled: false,
            skill_history_strategy: default_skill_history_strategy(),
            use_turn_context: false,
            trimmer: TrimmerConfigToml::default(),
            side_channel: SideChannelConfigToml::default(),
        }
    }
}

impl AppConfig {
    pub fn from_origin(origin: OriginAppConfig, config_dir: PathBuf) -> Self {
        Self {
            providers: origin.providers,
            llms: origin.llms,
            search: origin.search,
            tool: origin.tool,
            gateway: origin.gateway,
            voice: origin.voice,
            config_dir,
            config_path: origin.config_path,
        }
    }

    /// Return the skills directory. Defaults to `{config_dir}/skills`.
    pub fn skills_dir(&self) -> PathBuf {
        self.config_dir
            .join(self.tool.skills_dir.as_deref().unwrap_or("skills"))
    }

    /// Return the data directory for application runtime data.
    /// Defaults to `{config_dir}/data`.
    pub fn data_dir_path(&self) -> PathBuf {
        self.config_dir.join("data")
    }

    /// Return the prompts directory for agent template files.
    /// Defaults to `{config_dir}/prompts`.
    pub fn prompts_dir(&self) -> PathBuf {
        self.config_dir
            .join(self.tool.prompts_dir.as_deref().unwrap_or("prompts"))
    }

    /// Return the configured project context file path when provided.
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

    /// Return the path to the configuration file.
    /// Defaults to `{config_dir}/config.toml`.
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

    pub fn find_agent(&self, agent_id: &str) -> Result<&AgentSpec> {
        self.gateway
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not found", agent_id))
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
}

impl OriginAppConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let raw_config: RawAppConfig = toml::from_str(&content)?;
        let (mut config, warnings) = raw_config.migrate();
        config.apply_env_overrides();
        config.validate()?;
        for warning in warnings {
            log::warn!("{}", warning);
        }
        Ok(config)
    }

    fn apply_env_overrides(&mut self) {
        // if let Ok(api_key) = env::var("NOVA_API_KEY") {
        //     if !api_key.is_empty() {
        //         self.provider.api_key = api_key;
        //         if let Some(provider) = self.providers.get_mut(self.defaults.provider.as_str()) {
        //             provider.api_key = self.provider.api_key.clone();
        //         }
        //     }
        // }
        if let Ok(tavily_api_key) = env::var("TAVILY_API_KEY") {
            if !tavily_api_key.is_empty() {
                self.search.tavily_api_key = Some(tavily_api_key);
            }
        }
    }

    fn validate(&self) -> Result<()> {
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

        // if self.llm.model_config.thinking_budget.is_some() && self.llm.model_config.reasoning_effort.is_some() {
        //     bail!("llm.thinking_budget and llm.reasoning_effort cannot both be set");
        // }

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

#[derive(Debug, Deserialize, Default)]
struct RawAppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub llms: HashMap<String, RawRegisteredLlmConfig>,
    #[serde(default)]
    pub defaults: Option<DefaultBindingConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: RawToolConfig,
    #[serde(default)]
    pub gateway: RawGatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub config_path: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct RawToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    pub skills_dir: Option<String>,
    /// Prompts directory for agent template files. When None, defaults to `{config_dir}/prompts`.
    #[serde(default)]
    pub prompts_dir: Option<String>,
    /// 项目上下文文件路径。为空时按默认候选文件自动查找。
    #[serde(default)]
    pub project_context_file: Option<String>,
    /// 默认能力策略 ("minimal" | "full" | "workflow")。
    #[serde(default)]
    pub default_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLlmConfig {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(flatten)]
    model_config: RawModelConfig,
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
    #[serde(default)]
    agents: Vec<RawAgentSpec>,
    #[serde(default)]
    skill_routing_enabled: bool,
    #[serde(default = "default_skill_history_strategy")]
    skill_history_strategy: String,
    #[serde(default)]
    use_turn_context: bool,
    #[serde(default)]
    trimmer: RawTrimmerConfigToml,
    #[serde(default)]
    side_channel: SideChannelConfigToml,
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
    model_config: Option<AgentModelConfig>,
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
    fn migrate(self) -> (OriginAppConfig, Vec<String>) {
        let mut warnings = Vec::new();
        // let default_model = LlmConfig::default().model_config;
        // let use_named_bindings = !self.providers.is_empty() || !self.llms.is_empty() || self.defaults.is_some();
        // let legacy_provider_present = self.provider.is_some();
        // let legacy_llm_present = self.llm.is_some();
        // let mut legacy_provider = self.provider.unwrap_or_default();
        // let legacy_llm = migrate_legacy_llm(self.llm, &mut legacy_provider, &default_model, &mut warnings);
        //
        // let (providers, llms, defaults) = if use_named_bindings {
        //     if legacy_provider_present {
        //         warnings.push(
        //             "Detected legacy field provider together with providers; using providers and ignoring provider."
        //                 .to_string(),
        //         );
        //     }
        //     if legacy_llm_present {
        //         warnings.push(
        //             "Detected legacy field llm together with llms/defaults; using named llms and ignoring llm."
        //                 .to_string(),
        //         );
        //     }
        //
        //     let mut migrated_llms = HashMap::with_capacity(self.llms.len());
        //     for (llm_id, raw_llm) in self.llms {
        //         let mut model_config = raw_model_config_with_defaults(raw_llm.model_config, &default_model);
        //         if model_config.thinking_budget.is_some() && model_config.reasoning_effort.is_some() {
        //             model_config.reasoning_effort = None;
        //             warnings.push(format!(
        //                 "Both llms.{}.thinking_budget and llms.{}.reasoning_effort are set; preferring thinking_budget and ignoring reasoning_effort.",
        //                 llm_id, llm_id
        //             ));
        //         }
        //         migrated_llms.insert(
        //             llm_id,
        //             RegisteredLlmConfig {
        //                 provider: raw_llm.provider,
        //                 model_config,
        //             },
        //         );
        //     }
        //
        //     (self.providers, migrated_llms, self.defaults.unwrap_or_default())
        // } else {
        //     let providers = HashMap::from([(default_provider_binding_id(), legacy_provider.clone())]);
        //     let llms = HashMap::from([(
        //         default_llm_binding_id(),
        //         RegisteredLlmConfig {
        //             provider: default_provider_binding_id(),
        //             model_config: legacy_llm.model_config.clone(),
        //         },
        //     )]);
        //     (providers, llms, DefaultBindingConfig::default())
        // };

        let mut migrated_agents = Vec::with_capacity(self.gateway.agents.len());
        for mut agent in self.gateway.agents {
            if agent.prompt_file.is_none() && agent.prompt_inline.is_none() {
                if let Some(legacy_prompt) = agent.system_prompt_template.take() {
                    if looks_like_prompt_file(&legacy_prompt) {
                        agent.prompt_file = Some(legacy_prompt);
                        warnings.push(format!(
                            "Agent '{}' uses deprecated system_prompt_template; migrated to prompt_file.",
                            agent.id
                        ));
                    } else {
                        agent.prompt_inline = Some(legacy_prompt);
                        warnings.push(format!(
                            "Agent '{}' uses deprecated system_prompt_template; migrated to prompt_inline.",
                            agent.id
                        ));
                    }
                }
            }
            // todo
            let llm = self.llms.get(&agent.llm).unwrap();
            let model_config = crate::agent_catalog::ModelConfig {
                model: llm.model_config.model.clone(),
                temperature: llm.model_config.temperature,
                max_tokens: llm.model_config.max_tokens,
                // todo what is the default value
                top_p: llm.model_config.top_p.unwrap_or(1.0),
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
            });
        }

        let mut trimmer = TrimmerConfigToml {
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
            OriginAppConfig {
                providers: self.providers,
                llms: self.llms,
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
                    agents: migrated_agents,
                    skill_routing_enabled: self.gateway.skill_routing_enabled,
                    skill_history_strategy: self.gateway.skill_history_strategy,
                    use_turn_context: self.gateway.use_turn_context,
                    trimmer,
                    side_channel: self.gateway.side_channel,
                },
                voice: self.voice,
                config_path: self.config_path,
            },
            warnings,
        )
    }
}

fn raw_model_config_with_defaults(raw: RawModelConfig, default_model: &ModelConfig) -> ModelConfig {
    ModelConfig {
        provider: default_model.provider.clone(),
        model: raw.model,
        max_tokens: raw.max_tokens.unwrap_or(default_model.max_tokens),
        temperature: raw.temperature,
        top_p: raw.top_p,
        thinking_budget: raw.thinking_budget,
        reasoning_effort: raw.reasoning_effort,
    }
}

fn resolve_default_runtime_binding(
    providers: &HashMap<String, ProviderConfig>,
    llms: &HashMap<String, RegisteredLlmConfig>,
    defaults: &DefaultBindingConfig,
) -> Option<(ProviderConfig, LlmConfig)> {
    let provider = providers.get(defaults.provider.as_str())?.clone();
    let llm = llms.get(defaults.llm.as_str())?;
    let mut model_config = llm.model_config.clone();
    model_config.provider = Some(defaults.provider.clone());
    Some((provider, LlmConfig { model_config }))
}

fn migrate_legacy_llm(
    raw_llm: Option<RawLlmConfig>,
    provider: &mut ProviderConfig,
    default_model: &ModelConfig,
    warnings: &mut Vec<String>,
) -> LlmConfig {
    let mut llm = LlmConfig {
        model_config: default_model.clone(),
    };
    if let Some(raw_llm) = raw_llm {
        llm.model_config = raw_model_config_with_defaults(raw_llm.model_config, default_model);

        if !raw_llm.api_key.as_deref().unwrap_or_default().is_empty() {
            if provider.api_key.is_empty() {
                provider.api_key = raw_llm.api_key.unwrap_or_default();
                warnings.push("Detected deprecated field llm.api_key; migrated to provider.api_key.".to_string());
            } else {
                warnings.push(
                    "Both provider.api_key and deprecated llm.api_key exist; using provider.api_key.".to_string(),
                );
            }
        }
        if let Some(legacy_base_url) = raw_llm.base_url {
            if provider.base_url == default_base_url() {
                provider.base_url = legacy_base_url;
                warnings.push("Detected deprecated field llm.base_url; migrated to provider.base_url.".to_string());
            } else {
                warnings.push(
                    "Both provider.base_url and deprecated llm.base_url exist; using provider.base_url.".to_string(),
                );
            }
        }
    }

    if llm.model_config.thinking_budget.is_some() && llm.model_config.reasoning_effort.is_some() {
        llm.model_config.reasoning_effort = None;
        warnings.push(
            "Both llm.thinking_budget and llm.reasoning_effort are set; preferring thinking_budget and ignoring reasoning_effort."
                .to_string(),
        );
    }
    llm
}

fn looks_like_prompt_file(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.ends_with(".md") || trimmed.ends_with(".txt") || trimmed.contains('/') || trimmed.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, GatewayConfig, OriginAppConfig, RawAppConfig};
    use anyhow::Result;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn named_bindings_config_migrates_and_validates() {
        let toml = r#"
[providers.local]
api_key = "test-key"
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"
max_tokens = 4096

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "default"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate();
        config.validate().expect("config should validate");
        assert!(warnings.is_empty());
        assert_eq!(config.providers["local"].api_key, "test-key");
        assert_eq!(config.llms["local_default"].provider, "local");
        assert_eq!(config.defaults.provider, "local");
    }

    #[test]
    fn legacy_llm_api_key_migrates_to_provider() {
        let toml = r#"
[llm]
api_key = "old-key"
base_url = "http://old-host/v1"
model = "old-model"
max_tokens = 2048
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate();
        assert_eq!(config.providers["default"].api_key, "old-key");
        assert_eq!(config.llms["default"].provider, "default");
        assert!(!warnings.is_empty());
    }

    #[test]
    fn unknown_agent_provider_fails_validation() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "cloud2"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate();
        let error = config.validate().expect_err("config should fail validation");
        assert!(error.to_string().contains("unknown provider 'cloud2'"));
    }

    #[test]
    fn unknown_agent_llm_fails_validation() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "local"
llm = "missing"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate();
        let error = config.validate().expect_err("config should fail validation");
        assert!(error.to_string().contains("unknown llm 'missing'"));
    }

    #[test]
    fn mismatched_agent_llm_provider_fails_validation() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[providers.cloud]
base_url = "http://cloud.example/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[llms.cloud_default]
provider = "cloud"
model = "cloud-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "local"
llm = "cloud_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate();
        let error = config.validate().expect_err("config should fail validation");
        assert!(error
            .to_string()
            .contains("belongs to provider 'cloud', expected 'local'"));
    }

    #[test]
    fn prompt_file_and_inline_conflict_fails_validation() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "test"
display_name = "Test"
description = "test"
provider = "local"
llm = "local_default"
aliases = []
prompt_file = "test.md"
prompt_inline = "You are a test agent."
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate();
        assert!(config.validate().is_err());
    }

    #[test]
    fn named_bindings_override_legacy_fields() {
        let toml = r#"
[provider]
api_key = "legacy-key"
base_url = "http://legacy/v1"

[llm]
model = "legacy-model"

[providers.local]
api_key = "named-key"
base_url = "http://named/v1"

[llms.local_default]
provider = "local"
model = "named-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "default"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate();
        assert!(warnings.iter().any(|warning| warning.contains("ignoring provider")));
        assert!(warnings.iter().any(|warning| warning.contains("ignoring llm")));
    }

    #[test]
    fn legacy_trimmer_fields_migrate_correctly() {
        let toml = r#"
[gateway.trimmer]
max_history_tokens = 50000
preserve_recent = 5
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate();
        assert_eq!(config.gateway.trimmer.context_window, 58_192);
        assert_eq!(config.gateway.trimmer.output_reserve, 8_192);
        assert_eq!(config.gateway.trimmer.min_recent_messages, 5);
        assert!(config.gateway.trimmer.enabled);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn default_gateway_port_is_18801() {
        let config = GatewayConfig::default();
        assert_eq!(config.port, 18801);
    }

    #[test]
    fn skills_dir_defaults_to_workspace_nova_skills() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/skills"));
    }

    #[test]
    fn skills_dir_uses_relative_override_from_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.tool.skills_dir = Some("skills".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/skills"));
    }

    #[test]
    fn data_dir_defaults_to_workspace_nova_data() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/data"));
    }

    #[test]
    fn prompts_dir_defaults_to_workspace_prompts() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/prompts"));
    }

    #[test]
    fn prompts_dir_uses_relative_override_from_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.tool.prompts_dir = Some("templates".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/templates"));
    }

    #[test]
    fn prompts_dir_uses_absolute_path_directly() {
        let mut origin = OriginAppConfig::default();
        origin.tool.prompts_dir = Some("D:/etc/prompts".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/etc/prompts"));
    }

    #[test]
    fn project_context_file_uses_absolute_path_directly() {
        let mut origin = OriginAppConfig::default();
        origin.tool.project_context_file = Some("D:/etc/PROJECT.md".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.project_context_file(), Some(PathBuf::from("D:/etc/PROJECT.md")));
    }

    #[test]
    fn config_path_defaults_to_workspace_config_toml() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/config.toml"));
    }

    #[test]
    fn config_path_uses_relative_override_from_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.config_path = Some("custom.toml".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/custom.toml"));
    }

    #[test]
    fn config_path_uses_absolute_path_directly() {
        let mut origin = OriginAppConfig::default();
        origin.config_path = Some("D:/etc/app.toml".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/etc/app.toml"));
    }

    #[test]
    fn app_config_accepts_legacy_workspace_field() {
        let toml = r#"
workspace = "D:/legacy-workspace"
"#;
        let config: AppConfig = toml::from_str(toml).expect("app config should deserialize");
        assert_eq!(config.config_dir, PathBuf::from("D:/legacy-workspace"));
    }

    #[test]
    fn legacy_llm_api_key_is_migrated_to_provider() -> Result<()> {
        let raw = r#"
[llm]
api_key = "legacy-key"
model = "gpt-oss-120b"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
"#;
        let file = write_temp_config(raw)?;
        let config = OriginAppConfig::load_from_file(&file)?;
        let _ = std::fs::remove_file(&file);
        Ok(())
    }

    #[test]
    fn duplicate_agent_id_is_rejected() -> Result<()> {
        let raw = r#"
[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"

[[gateway.agents]]
id = "nova"
display_name = "Nova2"
description = "d2"
"#;
        let file = write_temp_config(raw)?;
        let error = OriginAppConfig::load_from_file(&file).expect_err("should reject duplicate id");
        let _ = std::fs::remove_file(&file);
        assert!(error.to_string().contains("duplicate agent id"));
        Ok(())
    }

    #[test]
    fn tavily_backend_without_api_key_is_rejected() -> Result<()> {
        let raw = r#"
[search]
backend = "tavily"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
"#;
        let file = write_temp_config(raw)?;
        let error = OriginAppConfig::load_from_file(&file).expect_err("should reject missing tavily key");
        let _ = std::fs::remove_file(&file);
        assert!(error.to_string().contains("tavily_api_key"));
        Ok(())
    }

    #[test]
    fn skills_dir_resolves_relative_to_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.tool.skills_dir = Some("my-skills".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/my-skills"));
    }

    fn write_temp_config(content: &str) -> Result<PathBuf> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("nova-agent-config-test-{}.toml", nanos));
        std::fs::write(&path, content)?;
        Ok(path)
    }
}
