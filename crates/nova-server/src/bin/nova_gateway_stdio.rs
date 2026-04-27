use clap::Parser;
use nova_app::bootstrap::build_application;
use nova_core::config::OriginAppConfig;
use nova_core::provider::openai_compat::OpenAiCompatClient;

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
    pub workspace: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // NOTE: Logger MUST go to stderr for stdio transport
    let _ = custom_utils::logger::logger_feature("nova-gateway-stdio", "debug", log::LevelFilter::Debug, false).build();

    let workspace = custom_utils::args::workspace(&args.workspace, ".nova")?;

    log::info!("Starting Nova Gateway Stdio...");

    let config_path = workspace.join("config.toml");
    let mut origin_config = OriginAppConfig::load_from_file(&config_path)?;

    // Apply CLI overrides
    if let Some(ref m) = args.model {
        origin_config.llm.model_config.model = m.clone();
    }
    origin_config.llm.model_config.max_tokens = args.max_tokens;
    if let Some(ref url) = args.base_url {
        origin_config.llm.base_url = url.clone();
    }

    let final_config = nova_core::config::AppConfig::from_origin(origin_config, workspace.clone());

    let client = OpenAiCompatClient::new(final_config.llm.api_key.clone(), final_config.llm.base_url.clone());
    let app = build_application(final_config, client).await?;

    nova_server_stdio::run_stdio(app).await
}
