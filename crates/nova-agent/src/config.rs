use crate::agent_catalog::ModelConfig as AgentModelConfig;
use crate::provider::ModelConfig;
use anyhow::{bail, Result};
use serde::de::{self, IgnoredAny};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_BINDING_PROVIDER: &str = "default";
const DEFAULT_BINDING_LLM: &str = "default";

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
    #[serde(default)]
    pub config_dir: PathBuf,
    /// Path to the configuration file relative to config_dir. When None, defaults to `config.toml`.
    #[serde(default)]
    pub config_path: Option<String>,
    /// 开发项目提示词文件列表，按优先级顺序。
    /// 相对路径相对于项目根目录解析。
    #[serde(default)]
    pub developer_prompt_files: Vec<String>,
    /// prompt 分层压缩配置。
    #[serde(default)]
    pub prompt_compaction: PromptCompactionConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptCompactionConfig {
    #[serde(default = "default_compaction_enabled")]
    pub enabled: bool,
    #[serde(default = "default_project_instruction_profile")]
    pub project_instruction_profile: String,
    #[serde(default = "default_skill_injection")]
    pub skill_injection: String,
    #[serde(default = "default_tool_guidance")]
    pub tool_guidance: String,
}

fn default_compaction_enabled() -> bool {
    true
}
fn default_project_instruction_profile() -> String {
    "auto".to_string()
}
fn default_skill_injection() -> String {
    "catalog".to_string()
}
fn default_tool_guidance() -> String {
    "compact".to_string()
}
fn default_max_tokens_field() -> String {
    "both".to_string()
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
                max_tokens_field: default_max_tokens_field(),
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            providers: default_provider_registry(),
            llms: default_llm_registry(),
            search: SearchConfig::default(),
            tool: ToolConfig::default(),
            gateway: GatewayConfig::default(),
            voice: VoiceConfig::default(),
            config_dir: PathBuf::new(),
            config_path: None,
            developer_prompt_files: Vec::new(),
            prompt_compaction: PromptCompactionConfig::default(),
        }
    }
}

impl Default for PromptCompactionConfig {
    fn default() -> Self {
        Self {
            enabled: default_compaction_enabled(),
            project_instruction_profile: default_project_instruction_profile(),
            skill_injection: default_skill_injection(),
            tool_guidance: default_tool_guidance(),
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
    /// 是否启用开发项目提示词读取。
    #[serde(default)]
    pub enable_project_developer_prompt: bool,
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
    #[serde(default = "default_max_tokens_field")]
    pub max_tokens_field: String,
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
    /// 循环保护配置（Plan 3 新增）。
    #[serde(default)]
    pub loop_guard: LoopGuardConfigToml,
    #[serde(default)]
    pub prompt_diagnostics: PromptDiagnosticsConfigToml,
    #[serde(default)]
    pub tool_result_compaction: ToolResultCompactionConfigToml,
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
fn default_loop_guard_enabled() -> bool {
    true
}
fn default_max_consecutive_duplicate_tool_calls() -> usize {
    2
}
fn default_max_stalled_iterations() -> usize {
    3
}
fn default_duplicate_read_mode() -> String {
    "warn_then_reject".to_string()
}
fn default_iteration_trim_ratio() -> f32 {
    0.85
}
fn default_prompt_diagnostics_enabled() -> bool {
    false
}
fn default_large_section_chars() -> usize {
    8_000
}
fn default_large_message_chars() -> usize {
    12_000
}
fn default_large_tool_result_chars() -> usize {
    8_000
}
fn default_tool_result_compaction_enabled() -> bool {
    true
}
fn default_tool_result_compaction_max_chars() -> usize {
    12_000
}
fn default_tool_result_compaction_head_chars() -> usize {
    4_000
}
fn default_tool_result_compaction_tail_chars() -> usize {
    4_000
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

/// 循环保护配置（TOML 序列化版本）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LoopGuardConfigToml {
    #[serde(default = "default_loop_guard_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_consecutive_duplicate_tool_calls")]
    pub max_consecutive_duplicate_tool_calls: usize,
    #[serde(default = "default_max_stalled_iterations")]
    pub max_stalled_iterations: usize,
    #[serde(default = "default_duplicate_read_mode")]
    pub duplicate_read_mode: String,
    #[serde(default = "default_iteration_trim_ratio")]
    pub iteration_trim_ratio: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptDiagnosticsConfigToml {
    #[serde(default = "default_prompt_diagnostics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_large_section_chars")]
    pub large_section_chars: usize,
    #[serde(default = "default_large_message_chars")]
    pub large_message_chars: usize,
    #[serde(default = "default_large_tool_result_chars")]
    pub large_tool_result_chars: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolResultCompactionConfigToml {
    #[serde(default = "default_tool_result_compaction_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tool_result_compaction_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_tool_result_compaction_head_chars")]
    pub head_chars: usize,
    #[serde(default = "default_tool_result_compaction_tail_chars")]
    pub tail_chars: usize,
    #[serde(default)]
    pub disable_for_tools: Vec<String>,
}

impl Default for ToolResultCompactionConfigToml {
    fn default() -> Self {
        Self {
            enabled: default_tool_result_compaction_enabled(),
            max_chars: default_tool_result_compaction_max_chars(),
            head_chars: default_tool_result_compaction_head_chars(),
            tail_chars: default_tool_result_compaction_tail_chars(),
            disable_for_tools: Vec::new(),
        }
    }
}

impl Default for PromptDiagnosticsConfigToml {
    fn default() -> Self {
        Self {
            enabled: default_prompt_diagnostics_enabled(),
            large_section_chars: default_large_section_chars(),
            large_message_chars: default_large_message_chars(),
            large_tool_result_chars: default_large_tool_result_chars(),
        }
    }
}

impl Default for LoopGuardConfigToml {
    fn default() -> Self {
        Self {
            enabled: default_loop_guard_enabled(),
            max_consecutive_duplicate_tool_calls: default_max_consecutive_duplicate_tool_calls(),
            max_stalled_iterations: default_max_stalled_iterations(),
            duplicate_read_mode: default_duplicate_read_mode(),
            iteration_trim_ratio: default_iteration_trim_ratio(),
        }
    }
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
            max_tokens_field: default_max_tokens_field(),
            agents: Vec::new(),
            skill_routing_enabled: false,
            skill_history_strategy: default_skill_history_strategy(),
            use_turn_context: false,
            trimmer: TrimmerConfigToml::default(),
            side_channel: SideChannelConfigToml::default(),
            loop_guard: LoopGuardConfigToml::default(),
            prompt_diagnostics: PromptDiagnosticsConfigToml::default(),
            tool_result_compaction: ToolResultCompactionConfigToml::default(),
        }
    }
}

impl AppConfig {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            ..Self::default()
        }
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P, config_dir: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let raw_config: RawAppConfig = toml::from_str(&content)?;
        let (mut config, warnings) = raw_config.migrate(config_dir);
        config.apply_env_overrides();
        config.validate()?;
        for warning in warnings {
            log::warn!("{}", warning);
        }
        Ok(config)
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

        // if self.llm.model_config.thinking_budget.is_some() && self.llm.model_config.reasoning_effort.is_some() {
        //     bail!("llm.thinking_budget and llm.reasoning_effort cannot both be set");
        // }

        // 检查 developer_prompt_files 中不包含空字符串
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

#[derive(Debug, Deserialize, Default)]
struct RawAppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub llms: HashMap<String, RawRegisteredLlmConfig>,
    #[serde(rename = "defaults", default, deserialize_with = "reject_removed_defaults")]
    _removed_defaults: Option<IgnoredAny>,
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
    #[serde(default)]
    pub developer_prompt_files: Vec<String>,
    #[serde(default)]
    pub prompt_compaction: PromptCompactionConfig,
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
    #[serde(default = "default_max_tokens_field")]
    max_tokens_field: String,
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
    model_config: Option<AgentModelConfig>,
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
    fn migrate(self, config_dir: PathBuf) -> (AppConfig, Vec<String>) {
        let mut warnings = Vec::new();
        let llms: HashMap<String, RegisteredLlmConfig> = self
            .llms
            .into_iter()
            .map(|(llm_id, raw_llm)| {
                let mut model_config = ModelConfig {
                    provider: Some(raw_llm.provider.clone()),
                    model: raw_llm.model_config.model,
                    max_tokens: raw_llm.model_config.max_tokens.unwrap_or(LlmConfig::default().model_config.max_tokens),
                    temperature: Some(raw_llm.model_config.temperature),
                    top_p: raw_llm.model_config.top_p,
                    thinking_budget: raw_llm.model_config.thinking_budget,
                    reasoning_effort: raw_llm.model_config.reasoning_effort,
                    max_tokens_field: self.gateway.max_tokens_field.clone(),
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
            let model_config = if let Some(model_config) = agent.model_config.take() {
                model_config
            } else if let Some(llm) = llms.get(&agent.llm) {
                crate::agent_catalog::ModelConfig {
                    model: llm.model_config.model.clone(),
                    temperature: llm.model_config.temperature.unwrap_or(0.0),
                    max_tokens: Some(llm.model_config.max_tokens),
                    top_p: llm.model_config.top_p.unwrap_or(1.0),
                }
            } else {
                crate::agent_catalog::ModelConfig {
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
                    use_turn_context: self.gateway.use_turn_context,
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
            },
            warnings,
        )
    }
}

fn looks_like_prompt_file(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.ends_with(".md") || trimmed.ends_with(".txt") || trimmed.contains('/') || trimmed.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, GatewayConfig, RawAppConfig};
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

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "default"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate(PathBuf::from("D:/workspace"));
        config.validate().expect("config should validate");
        assert!(warnings.is_empty());
        assert_eq!(config.providers["local"].api_key, "test-key");
        assert_eq!(config.llms["local_default"].provider, "local");
        assert_eq!(config.config_dir, PathBuf::from("D:/workspace"));
    }

    #[test]
    fn unknown_agent_provider_fails_validation() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "cloud2"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
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

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "local"
llm = "missing"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
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

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "test"
provider = "local"
llm = "cloud_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
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
        let (config, _) = raw.migrate(PathBuf::from("."));
        assert!(config.validate().is_err());
    }

    #[test]
    fn legacy_trimmer_fields_migrate_correctly() {
        let toml = r#"
[gateway.trimmer]
max_history_tokens = 50000
preserve_recent = 5
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, warnings) = raw.migrate(PathBuf::from("."));
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
        let config = AppConfig::new(PathBuf::from("D:/workspace"));
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/skills"));
    }

    #[test]
    fn skills_dir_uses_relative_override_from_workspace() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.tool.skills_dir = Some("skills".to_string());
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/skills"));
    }

    #[test]
    fn data_dir_defaults_to_workspace_nova_data() {
        let config = AppConfig::new(PathBuf::from("D:/workspace"));
        assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/data"));
    }

    #[test]
    fn prompts_dir_defaults_to_workspace_prompts() {
        let config = AppConfig::new(PathBuf::from("D:/workspace"));
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/prompts"));
    }

    #[test]
    fn prompts_dir_uses_relative_override_from_workspace() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.tool.prompts_dir = Some("templates".to_string());
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/workspace/templates"));
    }

    #[test]
    fn prompts_dir_uses_absolute_path_directly() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.tool.prompts_dir = Some("D:/etc/prompts".to_string());
        assert_eq!(config.prompts_dir(), PathBuf::from("D:/etc/prompts"));
    }

    #[test]
    fn project_context_file_uses_absolute_path_directly() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.tool.project_context_file = Some("D:/etc/PROJECT.md".to_string());
        assert_eq!(config.project_context_file(), Some(PathBuf::from("D:/etc/PROJECT.md")));
    }

    #[test]
    fn config_path_defaults_to_workspace_config_toml() {
        let config = AppConfig::new(PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/config.toml"));
    }

    #[test]
    fn config_path_uses_relative_override_from_workspace() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.config_path = Some("custom.toml".to_string());
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/custom.toml"));
    }

    #[test]
    fn config_path_uses_absolute_path_directly() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.config_path = Some("D:/etc/app.toml".to_string());
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
    fn app_config_loads_named_bindings_from_file() -> Result<()> {
        let raw = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "gpt-oss-120b"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let file = write_temp_config(raw)?;
        let config = AppConfig::load_from_file(&file, PathBuf::from("D:/workspace"))?;
        let _ = std::fs::remove_file(&file);
        assert_eq!(config.providers["local"].base_url, "http://localhost:8082/v1");
        Ok(())
    }

    #[test]
    fn defaults_section_is_rejected() {
        let raw = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[defaults]
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let error = toml::from_str::<RawAppConfig>(raw).expect_err("defaults section should be rejected");
        assert!(error.to_string().contains("[defaults] has been removed"));
    }

    #[test]
    fn duplicate_agent_id_is_rejected() -> Result<()> {
        let raw = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"

[[gateway.agents]]
id = "nova"
display_name = "Nova2"
description = "d2"
provider = "local"
llm = "local_default"
"#;
        let file = write_temp_config(raw)?;
        let error =
            AppConfig::load_from_file(&file, PathBuf::from("D:/workspace")).expect_err("should reject duplicate id");
        let _ = std::fs::remove_file(&file);
        assert!(error.to_string().contains("duplicate agent id"));
        Ok(())
    }

    #[test]
    fn tavily_backend_without_api_key_is_rejected() -> Result<()> {
        let raw = r#"
[search]
backend = "tavily"

[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let file = write_temp_config(raw)?;
        let error = AppConfig::load_from_file(&file, PathBuf::from("D:/workspace"))
            .expect_err("should reject missing tavily key");
        let _ = std::fs::remove_file(&file);
        assert!(error.to_string().contains("tavily_api_key"));
        Ok(())
    }

    #[test]
    fn skills_dir_resolves_relative_to_workspace() {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace"));
        config.tool.skills_dir = Some("my-skills".to_string());
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/my-skills"));
    }

    #[test]
    fn developer_prompt_files_empty_string_is_rejected() {
        let toml = r#"
developer_prompt_files = ["AGENTS.md", "", "DEVELOPER.md"]

[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        let error = config.validate().expect_err("config should fail validation");
        assert!(error.to_string().contains("developer_prompt_files[1] cannot be empty"));
    }

    #[test]
    fn developer_prompt_files_defaults_to_empty_list() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        assert!(config.developer_prompt_files.is_empty());
    }

    #[test]
    fn prompt_compaction_defaults_are_applied() {
        let config = AppConfig::default();
        assert!(config.prompt_compaction.enabled);
        assert_eq!(config.prompt_compaction.project_instruction_profile, "auto");
        assert_eq!(config.prompt_compaction.skill_injection, "catalog");
        assert_eq!(config.prompt_compaction.tool_guidance, "compact");
    }

    #[test]
    fn invalid_prompt_compaction_profile_is_rejected() {
        let toml = r#"
[prompt_compaction]
project_instruction_profile = "bad"

[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        let error = config.validate().expect_err("config should fail validation");
        assert!(error.to_string().contains("project_instruction_profile"));
    }

    #[test]
    fn agent_enable_project_developer_prompt_defaults_to_false() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        let agent = config.find_agent("nova").expect("agent should exist");
        assert!(!agent.enable_project_developer_prompt);
    }

    #[test]
    fn agent_enable_project_developer_prompt_can_be_set_to_true() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "d"
provider = "local"
llm = "local_default"
enable_project_developer_prompt = true
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        let agent = config.find_agent("developer").expect("agent should exist");
        assert!(agent.enable_project_developer_prompt);
    }

    #[test]
    fn loop_guard_defaults_are_applied() {
        let config = GatewayConfig::default();
        assert!(config.loop_guard.enabled);
        assert_eq!(config.loop_guard.max_consecutive_duplicate_tool_calls, 2);
        assert_eq!(config.loop_guard.max_stalled_iterations, 3);
        assert_eq!(config.loop_guard.duplicate_read_mode, "warn_then_reject");
    }

    #[test]
    fn loop_guard_warn_only_is_accepted() {
        let toml = r#"
[providers.local]
base_url = "http://localhost:8082/v1"

[llms.local_default]
provider = "local"
model = "test-model"

[gateway.loop_guard]
duplicate_read_mode = "warn_only"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "d"
provider = "local"
llm = "local_default"
"#;
        let raw: RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        config.validate().expect("config should validate");
    }

    #[test]
    fn prompt_diagnostics_defaults_are_applied() {
        let config = GatewayConfig::default();
        assert!(!config.prompt_diagnostics.enabled);
        assert_eq!(config.prompt_diagnostics.large_section_chars, 8_000);
        assert_eq!(config.prompt_diagnostics.large_message_chars, 12_000);
        assert_eq!(config.prompt_diagnostics.large_tool_result_chars, 8_000);
    }

    #[test]
    fn tool_result_compaction_defaults_are_applied() {
        let config = GatewayConfig::default();
        assert!(config.tool_result_compaction.enabled);
        assert_eq!(config.tool_result_compaction.max_chars, 12_000);
        assert_eq!(config.tool_result_compaction.head_chars, 4_000);
        assert_eq!(config.tool_result_compaction.tail_chars, 4_000);
        assert!(config.tool_result_compaction.disable_for_tools.is_empty());
    }

    fn write_temp_config(content: &str) -> Result<PathBuf> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("nova-agent-config-test-{}.toml", nanos));
        std::fs::write(&path, content)?;
        Ok(path)
    }
}
