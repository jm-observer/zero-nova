//! 配置数据模型、枚举和默认值构造。`r`n//!`r`n//! 此模块承载所有配置结构体、枚举和对应的 `Default` 实现，`r`n//! 确保模型层与加载/校验层职责分离。

use crate::agent_catalog::ModelConfig as AgentModelConfig;
use crate::provider::ModelConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_BINDING_PROVIDER: &str = "default";
const DEFAULT_BINDING_LLM: &str = "default";

// ---------------------------------------------------------------------------
//  Default helpers
// ---------------------------------------------------------------------------

pub(crate) fn default_provider_binding_id() -> String {
    DEFAULT_BINDING_PROVIDER.to_string()
}

pub(crate) fn default_llm_binding_id() -> String {
    DEFAULT_BINDING_LLM.to_string()
}

pub(crate) fn default_base_url() -> String {
    "http://127.0.0.1:8082/v1".to_string()
}

pub(crate) fn default_compaction_enabled() -> bool {
    true
}
pub(crate) fn default_project_instruction_profile() -> String {
    "auto".to_string()
}
pub(crate) fn default_skill_injection() -> String {
    "catalog".to_string()
}
pub(crate) fn default_tool_guidance() -> String {
    "compact".to_string()
}
pub(crate) fn default_max_tokens_field() -> String {
    "completion".to_string()
}

pub(crate) fn default_context_headers_enabled() -> bool {
    true
}

pub(crate) fn default_voice_enabled() -> bool {
    true
}
pub(crate) fn default_stt_model() -> String {
    "whisper-1".to_string()
}
pub(crate) fn default_tts_model() -> String {
    "tts-1".to_string()
}
pub(crate) fn default_tts_voice() -> String {
    "alloy".to_string()
}
pub(crate) fn default_stt_timeout_ms() -> u64 {
    30_000
}
pub(crate) fn default_tts_timeout_ms() -> u64 {
    30_000
}
pub(crate) fn default_voice_max_input_bytes() -> usize {
    5 * 1024 * 1024
}
pub(crate) fn default_voice_provider() -> String {
    "openai_compat".to_string()
}

pub(crate) fn default_host() -> String {
    "127.0.0.1".to_string()
}
pub(crate) fn default_port() -> u16 {
    18801
}
pub(crate) fn default_max_iterations() -> usize {
    30
}
pub(crate) fn default_subagent_timeout() -> u64 {
    300
}
pub(crate) fn default_max_tokens() -> usize {
    4096
}
pub(crate) fn default_skill_history_strategy() -> String {
    "global".to_string()
}
pub(crate) fn default_trimmer_enabled() -> bool {
    true
}
pub(crate) fn default_context_window() -> usize {
    128_000
}
pub(crate) fn default_output_reserve() -> usize {
    8_192
}
pub(crate) fn default_min_recent() -> usize {
    10
}
pub(crate) fn default_side_channel_enabled() -> bool {
    false
}
pub(crate) fn default_skill_reminder_interval() -> usize {
    5
}
pub(crate) fn default_loop_guard_enabled() -> bool {
    true
}
pub(crate) fn default_max_consecutive_duplicate_tool_calls() -> usize {
    2
}
pub(crate) fn default_max_stalled_iterations() -> usize {
    3
}
pub(crate) fn default_duplicate_read_mode() -> String {
    "warn_then_reject".to_string()
}
pub(crate) fn default_iteration_trim_ratio() -> f32 {
    0.85
}
pub(crate) fn default_prompt_diagnostics_enabled() -> bool {
    false
}
pub(crate) fn default_large_section_chars() -> usize {
    8_000
}
pub(crate) fn default_large_message_chars() -> usize {
    12_000
}
pub(crate) fn default_large_tool_result_chars() -> usize {
    8_000
}
pub(crate) fn default_tool_result_compaction_enabled() -> bool {
    true
}
pub(crate) fn default_tool_result_compaction_max_chars() -> usize {
    12_000
}
pub(crate) fn default_tool_result_compaction_head_chars() -> usize {
    4_000
}
pub(crate) fn default_tool_result_compaction_tail_chars() -> usize {
    4_000
}

pub(crate) fn default_provider_registry() -> HashMap<String, ProviderConfig> {
    HashMap::from([(default_provider_binding_id(), ProviderConfig::default())])
}

pub(crate) fn default_llm_registry() -> HashMap<String, RegisteredLlmConfig> {
    let default_model = LlmConfig::default().model_config;
    HashMap::from([(
        default_llm_binding_id(),
        RegisteredLlmConfig {
            provider: default_provider_binding_id(),
            model_config: default_model,
        },
    )])
}

// ---------------------------------------------------------------------------
//  顶层配置
// ---------------------------------------------------------------------------

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
    /// 出站请求 Header 注入配置。
    #[serde(default)]
    pub outbound_context_headers: OutboundContextHeaderConfig,
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

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct OutboundContextHeaderConfig {
    #[serde(default = "default_context_headers_enabled")]
    pub enabled: bool,
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

// ---------------------------------------------------------------------------
//  Default 实现
// ---------------------------------------------------------------------------

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
                extra_body: None,
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
            outbound_context_headers: OutboundContextHeaderConfig::default(),
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn default_gateway_port_is_18801() {
        let config = GatewayConfig::default();
        assert_eq!(config.port, 18801);
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

    #[test]
    fn prompt_compaction_defaults_are_applied() {
        let config = AppConfig::default();
        assert!(config.prompt_compaction.enabled);
        assert_eq!(config.prompt_compaction.project_instruction_profile, "auto");
        assert_eq!(config.prompt_compaction.skill_injection, "catalog");
        assert_eq!(config.prompt_compaction.tool_guidance, "compact");
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
        let raw: crate::config::RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
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
        let raw: crate::config::RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        let agent = config.find_agent("developer").expect("agent should exist");
        assert!(agent.enable_project_developer_prompt);
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
        let raw: crate::config::RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        config.validate().expect("config should validate");
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
        let raw: crate::config::RawAppConfig = toml::from_str(toml).expect("raw config should deserialize");
        let (config, _) = raw.migrate(PathBuf::from("."));
        assert!(config.developer_prompt_files.is_empty());
    }
}
