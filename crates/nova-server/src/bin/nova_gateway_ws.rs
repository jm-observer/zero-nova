use clap::Parser;
use custom_utils::{args::workspace as resolve_workspace, logger::logger_feature};
use nova_agent::app::bootstrap::build_application;
use nova_agent::config::AppConfig;
use std::{env::current_dir, future::pending, process::exit, time::Duration};
use sysinfo::{Pid, System};
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long)]
    pub host: Option<String>,

    #[arg(long)]
    pub port: Option<u16>,

    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub parent_pid: Option<u32>,

    #[arg(long)]
    pub workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let _ = logger_feature(
        "nova-gateway-ws",
        "debug,reqwest=info,sqlx=info,hyper_util=info,nova_agent::provider::openai_compat=info",
        log::LevelFilter::Debug,
        false,
    )
    .build();

    let workspace = resolve_workspace(&args.workspace, ".nova")?;

    log::info!("Working directory: {:?}", current_dir().unwrap_or_default());
    log::info!("Workspace directory: {:?}", workspace);

    let config_path = workspace.join("config.toml");
    log::info!("Attempting to load config from: {:?}", config_path);

    let mut final_config = AppConfig::load_from_file(&config_path, workspace.clone())?;
    if let Some(host) = &args.host {
        final_config.gateway.host = host.clone();
    }
    if let Some(port) = args.port {
        final_config.gateway.port = port;
    }
    let _ = final_config.selected_agent(args.agent.as_deref())?;

    log::info!("Starting Nova Gateway WS with config: {:?}", final_config);

    let addr = format!("{}:{}", final_config.gateway.host, final_config.gateway.port);
    let app = build_application(final_config).await?;

    tokio::select! {
        res = nova_server_ws::run_server(&addr, app) => {
            if let Err(e) = res {
                log::error!("Server error: {}", e);
                return Err(e);
            }
        }
        _ = async {
            if let Some(pid_val) = args.parent_pid {
                let mut sys = System::new();
                let pid = Pid::from(pid_val as usize);
                loop {
                    if !sys.refresh_process(pid) {
                        log::warn!("Detected parent process exit via PID monitoring (PID: {}).", pid_val);
                        exit(0);
                    }
                    sleep(Duration::from_secs(2)).await;
                }
            } else {
                pending::<()>().await
            }
        } => {}
        _ = async {
            use tokio::io::{AsyncReadExt, stdin};
            let mut stdin = stdin();
            let mut buf = [0u8; 1];
            loop {
                if stdin.read(&mut buf).await.unwrap_or(0) == 0 {
                    break;
                }
            }
        } => {
            log::warn!("Stdin closed (EOF). Parent process might have exited. Sidecar shutting down...");
            exit(0);
        }
    }

    Ok(())
}
