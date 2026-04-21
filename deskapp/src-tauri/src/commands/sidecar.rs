use crate::config::{AppConfig, SidecarManagementMode};
use log::{error, info};
use serde::Serialize;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
pub struct GatewayConfig {
    pub url: String,
    pub token: Option<String>,
}

#[derive(Serialize)]
pub struct SidecarExecutionContext {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

pub struct GatewaySidecar {
    child: Option<Child>,
}

impl GatewaySidecar {
    pub fn new() -> Self {
        Self { child: None }
    }
}

impl Default for GatewaySidecar {
    fn default() -> Self {
        Self::new()
    }
}

/// [COMPATIBILITY] 获取配置。前端仍调用 get_gateway_config，但返回的是 sidecar 的配置。
#[tauri::command]
pub async fn get_gateway_config(config: tauri::State<'_, AppConfig>) -> Result<GatewayConfig, String> {
    Ok(GatewayConfig {
        url: format!("ws://{}:{}", config.host, config.port),
        token: config.token.clone(),
    })
}

// =============================================================================
// CORE SIDE CAR LOGIC
// =============================================================================

/// 构建 Sidecar 运行所需的执行上下文 (路径, 参数, 工作目录)
fn build_sidecar_execution_context(app: &AppHandle) -> Result<SidecarExecutionContext, String> {
    let config = app.state::<AppConfig>();
    let config = config.inner();

    // 1. 获取基础命令
    let _name = config.sidecar.name.clone();
    let mut args = config.sidecar.args.clone().unwrap_or_default();
    let working_dir = config.config_dir.clone();

    // 2. 注入端口参数
    if let Some(arg_fmt) = &config.sidecar.port_arg {
        args.push(arg_fmt.clone());
        args.push(config.port.to_string());
    } else {
        args.push("--port".to_string());
        args.push(config.port.to_string());
    }

    // 2.2 注入 Workspace 参数
    args.push("--workspace".to_string());
    args.push(config.config_dir.to_string_lossy().to_string());

    // 3. 构建命令路径
    let mut cmd_path = config.sidecar.command.clone();
    if !std::path::Path::new(&cmd_path).is_absolute() {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let candidate = exe_dir.join(&cmd_path);
                if candidate.exists() {
                    cmd_path = candidate.to_string_lossy().to_string();
                }
            }
        }
    }

    // 4. 注入 Parent PID 用于生命周期绑定
    args.push("--parent-pid".to_string());
    args.push(std::process::id().to_string());

    Ok(SidecarExecutionContext {
        command: cmd_path,
        args,
        working_dir,
    })
}

pub fn start_gateway_sidecar(app: &AppHandle) -> Result<(), String> {
    let config = app.state::<AppConfig>();
    let config = config.inner();

    // 模式检查
    if config.sidecar.mode == SidecarManagementMode::Manual {
        return Err("Sidecar is in manual mode. Please manage the process externally.".to_string());
    }

    let state = app.state::<Mutex<GatewaySidecar>>();
    let mut sidecar = state.lock().map_err(|e| e.to_string())?;

    if sidecar.child.is_some() {
        return Ok(());
    }

    let ctx = build_sidecar_execution_context(app)?;

    info!("[Sidecar] Starting: {} {}", ctx.command, ctx.args.join(" "));
    info!("[Sidecar] Working Dir: {:?}", ctx.working_dir);

    let mut cmd = Command::new(&ctx.command);
    cmd.args(&ctx.args)
        .current_dir(&ctx.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| {
        let err_msg = format!(
            "Failed to spawn sidecar '{}' ({}): {}",
            config.sidecar.name, config.sidecar.command, e
        );
        error!("[Sidecar:ERR] {}", err_msg);
        err_msg
    })?;

    // 日志处理
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                info!("[Sidecar] {}", line);
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                error!("[Sidecar:ERR] {}", line);
            }
        });
    }

    sidecar.child = Some(child);
    Ok(())
}

pub fn stop_gateway_sidecar(app: &AppHandle) -> Result<(), String> {
    let config = app.state::<AppConfig>();
    if config.sidecar.mode == SidecarManagementMode::Manual {
        return Ok(());
    }

    let state = app.state::<Mutex<GatewaySidecar>>();
    let mut sidecar = state.lock().map_err(|e| e.to_string())?;

    if let Some(mut child) = sidecar.child.take() {
        let pid = child.id();
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            child.kill().map_err(|e| format!("Stop failed: {}", e))?;
        }
    }

    Ok(())
}

// =============================================================================
// [COMPATIBILITY] Tauri Commands
// =============================================================================

#[tauri::command]
pub async fn get_sidecar_execution_context(app: AppHandle) -> Result<SidecarExecutionContext, String> {
    build_sidecar_execution_context(&app)
}

#[tauri::command]
pub async fn start_gateway(app: AppHandle) -> Result<(), String> {
    start_gateway_sidecar(&app)
}

#[tauri::command]
pub async fn stop_gateway(app: AppHandle) -> Result<(), String> {
    stop_gateway_sidecar(&app)
}

#[tauri::command]
pub async fn restart_gateway(app: AppHandle) -> Result<(), String> {
    let config = app.state::<AppConfig>();
    if config.sidecar.mode == SidecarManagementMode::Manual {
        return Err("Restart not supported in manual mode.".to_string());
    }

    stop_gateway_sidecar(&app)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    start_gateway_sidecar(&app)
}
