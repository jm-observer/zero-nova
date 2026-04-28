use anyhow::anyhow;
use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum SidecarManagementMode {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SidecarConfig {
    /// 模式：auto (自动管理生命周期) 或 manual (用户自行管理)
    #[serde(default = "default_sidecar_mode")]
    pub mode: SidecarManagementMode,
    /// 可执行文件名 (例如: "my_agent" 或 "node")
    pub name: String,
    pub command: String,
    /// 启动参数
    pub args: Option<Vec<String>>,
    /// 端口参数的格式。例如: "--port" 或 "-p"。如果为 None，默认使用 "--port"。
    pub port_arg: Option<String>,
    /// workspace 路径参数名。例如 "--workspace"。若为 None，则不传递 workspace 参数给 sidecar。
    pub workspace_arg: Option<String>,
    /// 工作区路径 (Workspace)，传递给 nova-gateway 的 --workspace 参数
    pub workspace: Option<PathBuf>,
}

fn default_sidecar_mode() -> SidecarManagementMode {
    SidecarManagementMode::Auto
}

/// 应用配置（从 nova.yaml 和 server-config.json 读取）
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub config_dir: PathBuf,
    pub sidecar: SidecarConfig,
}

/// config.toml 中的配置
#[derive(Deserialize)]
struct TomlConfig {
    remote: RemoteConfig,
    sidecar: SidecarConfig,
}

#[derive(Deserialize, Default)]
struct RemoteConfig {
    host: Option<String>,
    port: Option<u16>,
    token: Option<String>,
}

/// 加载配置
pub fn load_config(_app: &tauri::AppHandle) -> anyhow::Result<AppConfig> {
    let default_workspace = if let Some(workspace) = custom_utils::args::arg_value("--workspace", "-w") {
        PathBuf::from(workspace).join(".nova")
    } else {
        // todo
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        home.join(".nova")
    };

    // 2. 尝试从工作区加载 config.toml
    let final_config_path = default_workspace.join("config.toml");

    if !final_config_path.exists() {
        return Err(anyhow!("找不到配置文件: {:?}", final_config_path));
    }

    info!("Loading config from: {:?}", final_config_path);
    let content = std::fs::read_to_string(&final_config_path)?;
    let toml_config: TomlConfig = toml::from_str(&content)?;

    let remote = toml_config.remote;
    let sidecar = toml_config.sidecar;

    Ok(AppConfig {
        host: remote.host.unwrap_or_else(|| "localhost".to_string()),
        port: remote.port.unwrap_or(18801),
        token: remote.token,
        config_dir: final_config_path
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or(anyhow!("final_config_path todo"))?,
        sidecar,
    })
}
