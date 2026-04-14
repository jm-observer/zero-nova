//! CLI for zero-nova library

use anyhow::{Context, Result};
use chrono::Local;
use clap::{Parser, Subcommand};
use colored::*;
use serde_json::json;
use tokio::sync::mpsc;
use zero_nova::{
    event::AgentEvent,
    mcp::client::McpClient,
    message::Message,
    provider::anthropic::AnthropicClient,
    provider::{LlmClient, ModelConfig},
    register_builtin_tools, AgentConfig, AgentRuntime, SystemPromptBuilder, ToolRegistry,
};

#[derive(Parser)]
#[command(name = "nova-cli", about = "Zero-Nova agent test CLI")]
struct Cli {
    /// Model name
    #[arg(long, default_value = "claude-sonnet-4-20250514")]
    model: String,
    /// Optional custom base URL for the LLM provider
    #[arg(long)]
    base_url: Option<String>,
    /// Verbose output (show tool inputs/outputs)
    #[arg(long)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Interactive conversation (REPL)
    Chat,
    /// One-shot execution
    Run {
        /// Prompt to execute
        prompt: String,
    },
    /// List registered tools
    Tools,
    /// Test MCP server connection
    McpTest {
        /// Command and args to start the MCP server
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = make_client(&cli)?;
    let mut tools = ToolRegistry::new();
    register_builtin_tools(&mut tools);

    // Build system prompt including loaded tools and environment information
    let prompt = SystemPromptBuilder::personal_assistant()
        .with_tools(&tools)
        .environment("date", current_date())
        .environment("platform", std::env::consts::OS)
        .build();

    let config = AgentConfig {
        model_config: ModelConfig {
            model: cli.model.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut agent = AgentRuntime::new(client, tools, prompt, config);

    match cli.command {
        Command::Chat => run_repl(&mut agent, cli.verbose).await?,
        Command::Run { prompt } => run_oneshot(&agent, &prompt, cli.verbose).await?,
        Command::Tools => {
            print_tools(&agent);
        }
        Command::McpTest { cmd } => test_mcp(&cmd).await?,
    }
    Ok(())
}

fn current_date() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn make_client(cli: &Cli) -> Result<impl LlmClient> {
    // Use Anthropic client; it reads ANTHROPIC_API_KEY from env.
    // If a custom base URL was supplied, use it, otherwise default.
    let base = cli
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("Environment variable ANTHROPIC_API_KEY not found. Please set it to your Anthropic API key.")?;
    let client = zero_nova::provider::anthropic::AnthropicClient::new(api_key, base);
    Ok(client)
}

async fn run_repl(agent: &mut AgentRuntime<impl LlmClient>, verbose: bool) -> Result<()> {
    let mut rl = rustyline::Editor::<()>::new();
    let mut history: Vec<Message> = Vec::new();
    loop {
        let line = match rl.readline("you> ") {
            Ok(l) => l,
            Err(_) => break,
        };
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        match input {
            "/quit" => break,
            "/help" => {
                println!("{}", "Available commands:".bold());
                println!("  /quit     - Exit the CLI");
                println!("  /help     - Show this help message");
                println!("  /tools    - List all registered tools");
                println!("  /clear    - Clear conversation history");
                println!("  /history  - Show conversation history stats");
                println!("  /prompt   - Show current system prompt");
                continue;
            }
            "/tools" => {
                print_tools(agent);
                continue;
            }
            "/clear" => {
                history.clear();
                println!("{}", "Conversation history cleared.".green());
                continue;
            }
            "/history" => {
                println!("{} messages in history", history.len());
                for (i, msg) in history.iter().enumerate() {
                    println!("  {}. [{:?}]", i + 1, msg.role);
                }
                continue;
            }
            "/prompt" => {
                println!("{}", "--- System Prompt ---".bright_black());
                println!("{}", agent.system_prompt());
                println!("{}", "---------------------".bright_black());
                continue;
            }
            _ => {
                let (tx, mut rx) = mpsc::channel(100);
                // Spawn a task to render events as they arrive
                let printer = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        render_event(&event, verbose);
                    }
                });
                let msgs = agent.run_turn(&history, input, tx.clone()).await?;
                drop(tx);
                printer.await.ok();
                for msg in msgs {
                    history.push(msg);
                }
            }
        }
    }
    Ok(())
}

async fn run_oneshot(agent: &AgentRuntime<impl LlmClient>, prompt: &str, verbose: bool) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);
    let printer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            render_event(&event, verbose);
        }
    });
    let _ = agent.run_turn(&[], prompt, tx.clone()).await?;
    drop(tx);
    printer.await.ok();
    Ok(())
}

fn print_tools(agent: &AgentRuntime<impl LlmClient>) {
    for def in agent.tools.tool_definitions() {
        println!("- {}: {}", def.name, def.description);
    }
}

async fn test_mcp(cmd: &[String]) -> Result<()> {
    if cmd.is_empty() {
        anyhow::bail!("No command provided for MCP test");
    }
    let command = &cmd[0];
    let args: Vec<&str> = cmd[1..].iter().map(|s| s.as_str()).collect();
    let client = McpClient::connect_stdio(command, &args).await?;
    let tools = client.list_tools().await?;
    println!("Available tools from MCP server:");
    for t in &tools {
        println!("- {}", t.name);
    }
    if let Some(first) = tools.first() {
        let result = client.call_tool(&first.name, json!({})).await?;
        println!("Tested tool '{}', result: {:?}", first.name, result);
    }
    Ok(())
}

fn render_event(event: &AgentEvent, verbose: bool) {
    match event {
        AgentEvent::TextDelta(text) => {
            print!("{text}");
        }
        AgentEvent::ToolStart { name, input, .. } => {
            if verbose {
                println!("\n{} {input:?}", format!("[tool: {name}]").cyan());
            } else {
                println!("\n{}", format!("[tool: {name}]").cyan());
            }
        }
        AgentEvent::ToolEnd {
            name, output, is_error, ..
        } => {
            if *is_error {
                println!("{}", format!("[tool: {name}] ERROR: {output}").red());
            } else if verbose {
                println!("{}", format!("[tool: {name}] OK: {output}").green());
            }
        }
        AgentEvent::TurnComplete { usage, .. } => {
            println!(
                "\n{}",
                format!("[tokens: input={}, output={}]", usage.input_tokens, usage.output_tokens).bright_black()
            );
        }
        AgentEvent::Error(e) => {
            eprintln!("\n{}", format!("[error] {e}").red().bold());
        }
        _ => {}
    }
}
