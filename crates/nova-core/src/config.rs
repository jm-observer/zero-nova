use crate::agent_catalog::ModelConfig as AgentModelConfig;
use crate::provider::ModelConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct OriginAppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: ToolConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    /// Application data directory. When None, defaults to `{workspace}/.nova/data`.
    #[serde(default)]
    pub data_dir: Option<String>,
    /// Path to the configuration file relative to workspace. When None, defaults to `config.toml`.
    #[serde(default)]
    pub config_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: ToolConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    pub workspace: PathBuf,
    /// Application data directory. When None, defaults to `{workspace}/.nova/data`.
    pub data_dir: Option<String>,
    /// Path to the configuration file relative to workspace. When None, defaults to `config.toml`.
    pub config_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(flatten)]
    pub model_config: ModelConfig,
}

fn default_base_url() -> String {
    "http://127.0.0.1:8082/v1".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: default_base_url(),
            model_config: ModelConfig {
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
    /// Prompts directory for agent template files. When None, defaults to `{workspace}/prompts`.
    #[serde(default)]
    pub prompts_dir: Option<String>,
    /// 默认能力策略 ("minimal" | "full" | "workflow")。
    /// Plan 1：基础扩展字段，不引入复杂嵌套。
    #[serde(default)]
    pub default_policy: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BashConfig {
    pub shell: Option<String>,
    pub sandbox: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AgentSpec {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub system_prompt_template: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub model_config: Option<AgentModelConfig>,
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
    9090
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
    pub fn from_origin(origin: OriginAppConfig, workspace: PathBuf) -> Self {
        Self {
            llm: origin.llm,
            search: origin.search,
            tool: origin.tool,
            gateway: origin.gateway,
            workspace,
            data_dir: origin.data_dir,
            config_path: origin.config_path,
        }
    }

    /// Resolve a path string against the workspace:
    /// - `None` returns the given default.
    /// - Absolute paths are returned as-is.
    /// - Relative paths are joined to `workspace`.
    fn resolve_path(&self, configured: Option<&str>, default: impl FnOnce() -> PathBuf) -> PathBuf {
        match configured {
            None => default(),
            Some(path) => {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    self.workspace.join(path)
                }
            }
        }
    }

    /// Return the skills directory. Defaults to `{workspace}/.nova/skills`.
    pub fn skills_dir(&self) -> PathBuf {
        self.resolve_path(self.tool.skills_dir.as_deref(), || {
            self.workspace.join(".nova").join("skills")
        })
    }

    /// Return the data directory for application runtime data.
    /// Defaults to `{workspace}/.nova/data`.
    pub fn data_dir_path(&self) -> PathBuf {
        self.resolve_path(self.data_dir.as_deref(), || self.workspace.join(".nova").join("data"))
    }

    /// Return the prompts directory for agent template files.
    /// Defaults to `{workspace}/prompts`.
    pub fn prompts_dir(&self) -> PathBuf {
        self.resolve_path(self.tool.prompts_dir.as_deref(), || self.workspace.join("prompts"))
    }

    /// Return the path to the configuration file.
    /// Defaults to `{workspace}/config.toml`.
    pub fn config_path(&self) -> PathBuf {
        self.resolve_path(self.config_path.as_deref(), || self.workspace.join("config.toml"))
    }
}

impl OriginAppConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: OriginAppConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, OriginAppConfig};
    use std::path::PathBuf;

    #[test]
    fn skills_dir_defaults_to_workspace_nova_skills() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/.nova/skills"));
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
        assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/.nova/data"));
    }

    #[test]
    fn data_dir_uses_relative_override_from_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.data_dir = Some("var".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.data_dir_path(), PathBuf::from("D:/workspace/var"));
    }

    #[test]
    fn data_dir_uses_absolute_path_directly() {
        let mut origin = OriginAppConfig::default();
        origin.data_dir = Some("D:/var/data".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.data_dir_path(), PathBuf::from("D:/var/data"));
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
    fn config_path_defaults_to_workspace_config_toml() {
        let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/config.toml"));
    }

    #[test]
    fn config_path_uses_relative_override_from_workspace() {
        let mut origin = OriginAppConfig::default();
        origin.config_path = Some("conf.toml".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/workspace/conf.toml"));
    }

    #[test]
    fn config_path_uses_absolute_path_directly() {
        let mut origin = OriginAppConfig::default();
        origin.config_path = Some("D:/etc/app.toml".to_string());
        let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
        assert_eq!(config.config_path(), PathBuf::from("D:/etc/app.toml"));
    }
}
