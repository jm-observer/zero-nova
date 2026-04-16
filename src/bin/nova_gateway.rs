//! Independent WebSocket gateway binary
use clap::Parser;
use zero_nova::gateway::start_server;
use zero_nova::provider::anthropic::AnthropicClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Host address
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port
    #[arg(long, default_value_t = 9090)]
    port: u16,

    /// Model name
    #[arg(long, default_value = "gpt-oss-120b")]
    model: String,

    /// Max tokens
    #[arg(long, default_value_t = 8192)]
    max_tokens: u32,

    /// Max iterations
    #[arg(long, default_value_t = 10)]
    max_iterations: usize,

    /// Base URL for LLM
    #[arg(long)]
    base_url: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger
    let _ = custom_utils::logger::logger_feature("nova-gateway", "debug", log::LevelFilter::Debug, true).build();

    log::info!(
        "Current working directory: {:?}",
        std::env::current_dir().unwrap_or_else(|e| std::path::PathBuf::from(e.to_string()))
    );
    log::info!(
        "Attempting to load config from: {:?}",
        std::env::current_dir().unwrap_or_default().join("config.toml")
    );

    let _args = Args::parse();

    let mut config = zero_nova::config::AppConfig::load_from_file("config.toml").unwrap_or_else(|e| {
        log::warn!("Failed to load config.toml: {}. Using default configuration.", e);
        zero_nova::config::AppConfig::default()
    });

    config.gateway.host = _args.host;
    config.gateway.port = _args.port;

    log::info!("Starting Nova Gateway {config:?}...");

    // Initialize client (using Anthropic as default for now)
    let client = AnthropicClient::from_config(&config.llm);

    // Start server
    start_server(config, client).await?;

    Ok(())
}
