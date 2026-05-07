use clap::Parser;
use custom_utils::{args::workspace as resolve_workspace, logger::logger_feature};
use nova_agent::app::bootstrap::build_application;
use nova_agent::config::{AppConfig, OriginAppConfig};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 18801)]
    pub port: u16,

    #[arg(long)]
    pub provider: Option<String>,

    #[arg(long)]
    pub llm: Option<String>,

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
    if let Some(provider) = &args.provider {
        origin_config.defaults.provider = provider.clone();
    }
    if let Some(llm) = &args.llm {
        origin_config.defaults.llm = llm.clone();
    }
    if let Some(ref m) = args.model {
        origin_config.llm.model_config.model = m.clone();
        if let Some(default_llm) = origin_config.llms.get_mut(origin_config.defaults.llm.as_str()) {
            default_llm.model_config.model = m.clone();
        }
    }
    origin_config.llm.model_config.max_tokens = args.max_tokens;
    if let Some(default_llm) = origin_config.llms.get_mut(origin_config.defaults.llm.as_str()) {
        default_llm.model_config.max_tokens = args.max_tokens;
    }
    if let Some(ref url) = args.base_url {
        origin_config.provider.base_url = url.clone();
        if let Some(default_provider) = origin_config
            .providers
            .get_mut(origin_config.defaults.provider.as_str())
        {
            default_provider.base_url = url.clone();
        }
    }

    let final_config = AppConfig::from_origin(origin_config, workspace.clone());
    let app = build_application(final_config).await?;

    nova_server_ws::run_stdio(app).await
}
