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
pub fn load_config(_app: &tauri::AppHandle) -> Result<AppConfig, Box<dyn std::error::Error>> {
    // 1. 尝试路径 1：可执行文件同级目录 (生产环境)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let config_path_1 = exe_dir.join("config.toml");

    // 2. 尝试路径 2：项目根目录下的 .nova/ 目录 (开发环境回退)
    // 假设开发时的结构是 deskapp/src-tauri/target/debug/xxx.exe
    // 我们向上找几层以寻找 .nova 目录
    let mut config_path_2 = exe_dir.clone();
    let mut found_dev_config = false;
    for _ in 0..5 {
        let dev_nova_path = config_path_2.join(".nova").join("config.toml");
        if dev_nova_path.exists() {
            config_path_2 = dev_nova_path;
            found_dev_config = true;
            break;
        }
        if let Some(parent) = config_path_2.parent() {
            config_path_2 = parent.to_path_buf();
        } else {
            break;
        }
    }

    let final_config_path = if config_path_1.exists() {
        config_path_1
    } else if found_dev_config {
        info!("Falling back to dev config at: {:?}", config_path_2);
        config_path_2
    } else {
        return Err(format!("找不到配置文件 config.toml (已尝试 {:?} 和 .nova 目录)", config_path_1).into());
    };

    info!("Loading config from: {:?}", final_config_path);
    let content = std::fs::read_to_string(&final_config_path)?;
    let toml_config: TomlConfig = toml::from_str(&content)?;

    let remote = toml_config.remote;
    let sidecar = toml_config.sidecar;

    Ok(AppConfig {
        host: remote.host.unwrap_or_else(|| "localhost".to_string()),
        port: remote.port.unwrap_or(18801),
        token: remote.token,
        config_dir: final_config_path.parent().unwrap_or(&exe_dir).to_path_buf(),
        sidecar,
    })
}
