//! Independent WebSocket gateway binary
use clap::Parser;
use std::env;
use zero_nova::gateway::{start_server, GatewayConfig};
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
    let _ = custom_utils::logger::logger_feature("nova-gateway", "info", log::LevelFilter::Info, true).build();
    log::info!("Starting Nova Gateway...");

    let args = Args::parse();

    let config = GatewayConfig {
        host: args.host,
        port: args.port,
        model: args.model,
        max_tokens: args.max_tokens,
        max_iterations: args.max_iterations,
        api_key: env::var("ANTHROPIC_API_KEY").ok(),
        base_url: args.base_url,
    };

    // Initialize client (using Anthropic as default for now)
    let api_key = config
        .api_key
        .clone()
        .unwrap_or_else(|| env::var("API_KEY").unwrap_or_default());

    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());

    let client = AnthropicClient::new(api_key, base_url);

    // Start server
    start_server(config, client).await?;

    Ok(())
}
