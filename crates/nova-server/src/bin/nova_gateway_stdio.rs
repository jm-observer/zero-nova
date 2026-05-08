use clap::Parser;
use custom_utils::{args::workspace as resolve_workspace, logger::logger_feature};
use nova_agent::app::bootstrap::build_application;
use nova_agent::config::AppConfig;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 18801)]
    pub port: u16,

    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // NOTE: Logger MUST go to stderr for stdio transport
    let _ = logger_feature("nova-gateway-stdio", "debug", log::LevelFilter::Debug, false).build();

    let workspace = resolve_workspace(&args.workspace, ".nova")?;

    log::info!("Starting Nova Gateway Stdio...");

    let config_path = workspace.join("config.toml");
    let final_config = AppConfig::load_from_file(&config_path, workspace.clone())?;
    let _ = final_config.selected_agent(args.agent.as_deref())?;
    let app = build_application(final_config).await?;

    nova_server_ws::run_stdio(app).await
}
