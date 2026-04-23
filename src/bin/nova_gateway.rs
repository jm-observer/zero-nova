//! Independent WebSocket gateway binary
use clap::Parser;
use sysinfo::{Pid, System};
use zero_nova::app::bootstrap::bootstrap;
use zero_nova::config::AppConfig;
use zero_nova::provider::openai_compat::OpenAiCompatClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Host address
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port
    #[arg(long, default_value_t = 9090)]
    port: u16,

    /// Model name, default_value = "gpt-oss-120b"
    #[arg(long)]
    model: Option<String>,

    /// Max tokens
    #[arg(long, default_value_t = 8192)]
    max_tokens: u32,

    /// Base URL for LLM
    #[arg(long)]
    base_url: Option<String>,

    /// Parent PID for lifecycle management
    #[arg(long)]
    parent_pid: Option<u32>,
    /// Optional workspace directory for config and prompts
    #[arg(long)]
    workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger
    let _ = custom_utils::logger::logger_feature("nova-gateway", "debug", log::LevelFilter::Debug, false).build();

    let _args = Args::parse();

    let workspace = custom_utils::args::workspace(&_args.workspace, ".nova")?;

    log::info!("Working directory: {:?}", std::env::current_dir().unwrap_or_default());
    log::info!("Workspace directory: {:?}", workspace);

    let config_path = workspace.join("config.toml");
    log::info!("Attempting to load config from: {:?}", config_path);

    let mut config = zero_nova::config::OriginAppConfig::load_from_file(&config_path)?;

    config.gateway.host = _args.host;
    config.gateway.port = _args.port;
    if let Some(model) = _args.model {
        config.llm.model_config.model = model;
    }

    config.llm.model_config.max_tokens = _args.max_tokens;

    if let Some(base_url) = _args.base_url {
        config.llm.base_url = base_url;
    }

    log::info!("Starting Nova Gateway {config:?}...");

    // Initialize client (using Anthropic as default for now)
    // let client = OpenAiCompatClient::from_config(&config.llm);
    let client = OpenAiCompatClient::new(config.llm.api_key.clone(), config.llm.base_url.clone());

    // Use tokio::select! to run the server and monitor parent process or stdin
    tokio::select! {
        // Task 1: Run the server
        res = async {
            bootstrap(AppConfig::from_origin(config, workspace), client, workspace).await
        } => {
            if let Err(e) = res {
                log::error!("Server error: {}", e);
                return Err(e);
            }
        }
        // Task 2: PID monitoring (Strategy A)
        _ = async {
            if let Some(pid_val) = _args.parent_pid {
                let mut sys = System::new();
                let pid = Pid::from(pid_val as usize);
                loop {
                    // refresh_process returns false if the process does not exist or fails to refresh
                    if !sys.refresh_process(pid) {
                        log::warn!("Detected parent process exit via PID monitoring (PID: {}).", pid_val);
                        std::process::exit(0);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            } else {
                std::future::pending::<()>().await
            }
        } => {}
        // Task 3: Monitor stdin for EOF (Strategy B)
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
            std::process::exit(0);
        }
    }

    Ok(())
}
