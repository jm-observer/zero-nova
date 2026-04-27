use clap::Parser;
use custom_utils::{args::workspace as resolve_workspace, logger::logger_feature};
use nova_agent::app::bootstrap::build_application;
use nova_agent::config::{AppConfig, OriginAppConfig};
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use std::{env::current_dir, future::pending, process::exit, time::Duration};
use sysinfo::{Pid, System};
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 9090)]
    pub port: u16,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long, default_value_t = 8192)]
    pub max_tokens: u32,

    #[arg(long)]
    pub base_url: Option<String>,

    #[arg(long)]
    pub parent_pid: Option<u32>,

    #[arg(long)]
    pub workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let _ = logger_feature("nova-gateway-ws", "debug", log::LevelFilter::Debug, false).build();

    let workspace = resolve_workspace(&args.workspace, ".nova")?;

    log::info!("Working directory: {:?}", current_dir().unwrap_or_default());
    log::info!("Workspace directory: {:?}", workspace);

    let config_path = workspace.join("config.toml");
    log::info!("Attempting to load config from: {:?}", config_path);

    let mut origin_config = OriginAppConfig::load_from_file(&config_path)?;

    // Keep CLI flags as the highest priority so one-off runs do not require editing config files.
    if let Some(ref m) = args.model {
        origin_config.llm.model_config.model = m.clone();
    }
    origin_config.llm.model_config.max_tokens = args.max_tokens;
    if let Some(ref url) = args.base_url {
        origin_config.llm.base_url = url.clone();
    }
    origin_config.gateway.host = args.host.clone();
    origin_config.gateway.port = args.port;

    let final_config = AppConfig::from_origin(origin_config.clone(), workspace.clone());

    log::info!("Starting Nova Gateway WS with config: {:?}", final_config);

    let client = OpenAiCompatClient::new(final_config.llm.api_key.clone(), final_config.llm.base_url.clone());
    let app = build_application(final_config, client).await?;

    let addr = format!("{}:{}", args.host, args.port);

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
