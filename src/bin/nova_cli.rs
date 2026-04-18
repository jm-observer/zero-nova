//! CLI for zero-nova library

use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand};
use colored::Colorize;
use rustyline::history::FileHistory;
use serde_json::json;
use std::io::Write;
use tokio::sync::mpsc;
use zero_nova::agent::{AgentConfig, AgentRuntime};
use zero_nova::event::AgentEvent;
use zero_nova::mcp::client::McpClient;
use zero_nova::message::Message;
use zero_nova::prompt::SystemPromptBuilder;
use zero_nova::provider::LlmClient;

use zero_nova::tool::{builtin::register_builtin_tools, ToolRegistry};

#[derive(Parser)]
#[command(name = "nova-cli", about = "Zero-Nova agent test CLI")]
struct Cli {
    /// Model name
    #[arg(long, default_value = "gpt-oss-120b", global = true)]
    model: String,
    /// Optional custom base URL for the LLM provider
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// Optional workspace directory for config and prompts
    #[arg(long, global = true)]
    workspace: Option<std::path::PathBuf>,
    /// Verbose output (show tool inputs/outputs)
    #[arg(long, global = true)]
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
    let _ =
        custom_utils::logger::logger_feature("nova_cli", "debug,rustyline=info", log::LevelFilter::Info, false).build();

    let config_path = cli
        .workspace
        .as_ref()
        .map(|w| w.join("config.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("config.toml"));

    let config = zero_nova::config::AppConfig::load_from_file(config_path.to_str().unwrap_or("config.toml"))
        .unwrap_or_else(|e| {
            log::warn!("Failed to load {:?}: {}. Using default configuration.", config_path, e);
            zero_nova::config::AppConfig::default()
        });

    log::info!("Starting Nova CLI with model: {}", config.llm.model_config.model);

    let client = zero_nova::provider::anthropic::AnthropicClient::from_config(&config.llm);
    let mut tools = ToolRegistry::new();
    register_builtin_tools(&mut tools, &config);

    // Build system prompt including loaded tools and environment information
    let prompt_builder = if let Some(ref workspace) = cli.workspace {
        SystemPromptBuilder::new_from_path(workspace)
    } else {
        SystemPromptBuilder::new()
    };

    let prompt = prompt_builder
        .with_tools(&tools)
        .environment("date", current_date())
        .environment("platform", std::env::consts::OS)
        .build();

    let agent_config = AgentConfig {
        max_iterations: 5,
        model_config: config.llm.model_config.clone(),
        tool_timeout: std::time::Duration::from_secs(120),
    };

    let mut agent = AgentRuntime::new(client, tools, prompt, agent_config);

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

/// Runs the REPL loop for interactive chat.
async fn run_repl(agent: &mut AgentRuntime<impl LlmClient>, verbose: bool) -> Result<()> {
    let mut rl = rustyline::Editor::<(), FileHistory>::new()?;
    let mut history: Vec<Message> = Vec::new();
    while let Ok(line) = rl.readline("you> ") {
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
                let printer = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        render_event(&event, verbose);
                    }
                });

                tokio::select! {
                    result = agent.run_turn(&history, input, tx.clone(), None) => {
                        drop(tx);
                        printer.await.ok();
                        match result {
                            Ok(turn_result) => {
                                println!();
                                for msg in turn_result.messages {
                                    history.push(msg);
                                }
                            }
                            Err(e) => {
                                eprintln!("\n{}", format!("[error] {}", e).red());
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        printer.abort();
                        println!("\n{}", "Interrupted by user.".yellow());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Executes a one-shot interaction with the given prompt.
async fn run_oneshot(agent: &AgentRuntime<impl LlmClient>, prompt: &str, verbose: bool) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);
    let printer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            render_event(&event, verbose);
        }
    });
    let _ = agent.run_turn(&[], prompt, tx.clone(), None).await?;
    drop(tx);
    printer.await.ok();
    Ok(())
}

/// Prints the list of available tools.
fn print_tools(agent: &AgentRuntime<impl LlmClient>) {
    for def in agent.tools().tool_definitions() {
        println!("- {}: {}", def.name, def.description);
    }
}

/// Tests the MCP server by invoking the first tool.
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

/// Renders an `AgentEvent` to the console, optionally verbose.
fn render_event(event: &AgentEvent, verbose: bool) {
    match event {
        AgentEvent::TextDelta(text) => {
            print!("{text}");
            let _ = std::io::stdout().flush();
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
        AgentEvent::IterationLimitReached { iterations } => {
            println!(
                "\n{}",
                format!("[warn] iteration limit reached ({iterations} iterations)").yellow()
            );
        }
        AgentEvent::Error(e) => {
            eprintln!("\n{}", format!("[error] {e}").red().bold());
        }
    }
}

fn current_date() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
