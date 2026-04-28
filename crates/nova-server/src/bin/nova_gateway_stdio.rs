use clap::Parser;
use custom_utils::{args::workspace as resolve_workspace, logger::logger_feature};
use nova_agent::app::bootstrap::build_application;
use nova_agent::config::{AppConfig, OriginAppConfig};
use nova_agent::provider::openai_compat::OpenAiCompatClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 18801)]
    pub port: u16,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long, default_value_t = 8192)]
    pub max_tokens: u32,

    #[arg(long)]
    pub base_url: Option<String>,

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
    let mut origin_config = OriginAppConfig::load_from_file(&config_path)?;

    // Keep CLI flags as the highest priority so one-off runs do not require editing config files.
    if let Some(ref m) = args.model {
        origin_config.llm.model_config.model = m.clone();
    }
    origin_config.llm.model_config.max_tokens = args.max_tokens;
    if let Some(ref url) = args.base_url {
        origin_config.provider.base_url = url.clone();
    }

    let final_config = AppConfig::from_origin(origin_config, workspace.clone());

    let client = OpenAiCompatClient::new(
        final_config.provider.api_key.clone(),
        final_config.provider.base_url.clone(),
    );
    let app = build_application(final_config, client).await?;

    nova_server_ws::run_stdio(app).await
}
