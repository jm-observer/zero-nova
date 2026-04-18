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
    /// 工作目录
    pub working_dir: Option<PathBuf>,
    /// 端口参数的格式。例如: "--port" 或 "-p"。如果为 None，默认使用 "--port"。
    pub port_arg: Option<String>,
}

fn default_sidecar_mode() -> SidecarManagementMode {
    SidecarManagementMode::Auto
}

/// 应用配置（从 openflux.yaml 和 server-config.json 读取）
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub config_dir: PathBuf,
    pub sidecar: SidecarConfig,
}

/// openflux.yaml 中的配置
#[derive(Deserialize)]
struct YamlConfig {
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
pub fn load_config(_app: &tauri::AppHandle) -> Result<AppConfig, Box<dyn std::error::Error>> {
    // 默认配置目录：可执行文件同级
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // 尝试读取 openflux.yaml
    let yaml_path = exe_dir.join("openflux.yaml");
    info!("{yaml_path:?} exists={}", yaml_path.exists());
    let content = std::fs::read_to_string(&yaml_path)?;
    let yaml_config: YamlConfig = serde_yaml::from_str(&content).unwrap();

    let remote = yaml_config.remote;

    // 如果没有配置 sidecar，提供一个默认值（指向系统 node 或用户指定的默认值）
    let sidecar = yaml_config.sidecar;

    Ok(AppConfig {
        host: remote.host.unwrap_or_else(|| "localhost".to_string()),
        port: remote.port.unwrap_or(18801),
        token: remote.token,
        config_dir: exe_dir,
        sidecar,
    })
}
