//! CLI for zero-nova library

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use log::info;
use nova_core::agent::{AgentConfig, AgentRuntime};
use nova_core::event::AgentEvent;
use nova_core::mcp::client::McpClient;
use nova_core::message::Message;
use nova_core::prompt::SystemPromptBuilder;
use nova_core::provider::openai_compat::OpenAiCompatClient;
use nova_core::provider::LlmClient;
use nova_core::tool::{builtin::register_builtin_tools, ToolRegistry};
use rustyline::history::FileHistory;
use serde_json::json;
use std::io::Write;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
enum OutputFormat {
    #[default]
    PlainText,
    StreamJson,
}

#[derive(Parser)]
#[command(name = "nova-cli", about = "Zero-Nova agent test CLI", version)]
struct Cli {
    /// Model name
    #[arg(long, global = true)]
    model: Option<String>,
    /// Optional custom base URL for the LLM provider
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// Optional workspace directory for config and prompts
    #[arg(long, global = true)]
    workspace: Option<String>,
    /// Verbose output (show tool inputs/outputs)
    #[arg(long, global = true)]
    verbose: bool,
    /// Output format
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::PlainText)]
    output_format: OutputFormat,
    /// Include a specific skill directory
    #[arg(long, global = true)]
    include_skill: Option<String>,
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

    let workspace = custom_utils::args::workspace(&cli.workspace, ".nova")?;
    info!("workspace {}", workspace.display());
    let config_path = workspace.join("config.toml");

    let mut config = nova_core::config::OriginAppConfig::load_from_file(&config_path)?;

    if let Some(model) = &cli.model {
        config.llm.model_config.model = model.to_string();
    }
    if let Some(base_url) = &cli.base_url {
        config.llm.base_url = base_url.to_string();
    }

    let config = nova_core::config::AppConfig::from_origin(config, workspace.clone());

    log::info!("Starting Nova CLI with : {:?}", config);
    let client = OpenAiCompatClient::new(config.llm.api_key.clone(), config.llm.base_url.clone());
    let mut tools = ToolRegistry::new();
    register_builtin_tools(&mut tools, &config);

    // Build system prompt including loaded tools and environment information
    let prompt_builder = SystemPromptBuilder::new();

    let system_prompt_str = prompt_builder.with_tools(&tools).build();

    let agent_config = AgentConfig {
        max_iterations: 15, // Increase for skill evaluation tasks
        model_config: config.llm.model_config.clone(),
        tool_timeout: std::time::Duration::from_secs(300),
    };

    // Skills handling
    let mut skill_registry = nova_core::skill::SkillRegistry::new();

    // Load skills from the default workspace location
    let skill_dir = workspace.join(".nova").join("skills");
    if let Err(e) = skill_registry.load_from_dir(&skill_dir) {
        if matches!(cli.output_format, OutputFormat::PlainText) {
            log::warn!("Failed to load skills from {:?}: {}", skill_dir, e);
        }
    }

    // Additionally include a specific skill if provided via --include-skill
    if let Some(extra_skill_path) = &cli.include_skill {
        let path = std::path::Path::new(extra_skill_path);
        if let Err(e) = skill_registry.load_single_skill(path) {
            log::error!("Failed to load included skill from {:?}: {}", path, e);
        }
    }

    let skill_prompt = skill_registry.generate_system_prompt();
    let final_system_prompt = format!("{}\n\n{}", system_prompt_str, skill_prompt);

    let mut agent = AgentRuntime::new(client, tools, agent_config);

    match cli.command {
        Command::Chat => run_repl(&mut agent, &final_system_prompt, cli.verbose, cli.output_format).await?,
        Command::Run { prompt } => {
            run_oneshot(&agent, &final_system_prompt, &prompt, cli.verbose, cli.output_format).await?
        }
        Command::Tools => {
            print_tools(&agent);
        }
        Command::McpTest { cmd } => test_mcp(&cmd).await?,
    }
    Ok(())
}

/// Runs the REPL loop for interactive chat.
async fn run_repl(
    agent: &mut AgentRuntime<impl LlmClient>,
    system_prompt: &str,
    verbose: bool,
    format: OutputFormat,
) -> Result<()> {
    let mut rl = rustyline::Editor::<(), FileHistory>::new()?;
    let mut history: Vec<Message> = Vec::new();

    // Initialize history with system prompt
    if !system_prompt.is_empty() {
        history.push(Message {
            role: nova_core::message::Role::System,
            content: vec![nova_core::message::ContentBlock::Text {
                text: system_prompt.to_string(),
            }],
        });
    }

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
                println!("  /clear    - Clear conversation history (keeps system prompt)");
                println!("  /history  - Show conversation history stats");
                println!("  /prompt   - Show current system prompt");
                continue;
            }
            "/tools" => {
                print_tools(agent);
                continue;
            }
            "/clear" => {
                // Keep the first system message if it exists
                let system_msg = history
                    .first()
                    .cloned()
                    .filter(|m| m.role == nova_core::message::Role::System);
                history.clear();
                if let Some(msg) = system_msg {
                    history.push(msg);
                }
                println!("{}", "Conversation history cleared (system prompt preserved).".green());
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
                if let Some(msg) = history.first().filter(|m| m.role == nova_core::message::Role::System) {
                    for block in &msg.content {
                        if let nova_core::message::ContentBlock::Text { text } = block {
                            println!("{}", text);
                        }
                    }
                } else {
                    println!("(No system prompt set)");
                }
                println!("{}", "---------------------".bright_black());
                continue;
            }
            _ => {
                let printer_instance = EventPrinter::new(verbose, format);
                let (tx, mut rx) = mpsc::channel(100);
                let printer_task = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        printer_instance.render(&event);
                    }
                });

                tokio::select! {
                    result = agent.run_turn(&history, input, tx.clone(), None) => {
                        drop(tx);
                        printer_task.await.ok();
                        match result {
                            Ok(turn_result) => {
                                if matches!(format, OutputFormat::PlainText) {
                                    println!();
                                }
                                for msg in turn_result.messages {
                                    history.push(msg);
                                }
                            }
                            Err(e) => {
                                EventPrinter::new(verbose, format).print_error(&e);
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        printer_task.abort();
                        println!("\n{}", "Interrupted by user.".yellow());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Executes a one-shot interaction with the given prompt.
async fn run_oneshot(
    agent: &AgentRuntime<impl LlmClient>,
    system_prompt: &str,
    user_input: &str,
    verbose: bool,
    format: OutputFormat,
) -> Result<()> {
    let printer = EventPrinter::new(verbose, format);
    let (tx, mut rx) = mpsc::channel(100);

    let printer_task = tokio::spawn(async move {
        let internal_printer = EventPrinter::new(verbose, format);
        while let Some(event) = rx.recv().await {
            internal_printer.render(&event);
        }
    });

    let mut history = Vec::new();
    if !system_prompt.is_empty() {
        history.push(Message {
            role: nova_core::message::Role::System,
            content: vec![nova_core::message::ContentBlock::Text {
                text: system_prompt.to_string(),
            }],
        });
    }

    let result = agent.run_turn(&history, user_input, tx, None).await;
    printer_task.await.ok();

    if let Err(e) = result {
        printer.print_error(&e);
        return Err(e);
    }

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

struct EventPrinter {
    verbose: bool,
    format: OutputFormat,
}

impl EventPrinter {
    fn new(verbose: bool, format: OutputFormat) -> Self {
        Self { verbose, format }
    }

    fn render(&self, event: &AgentEvent) {
        match self.format {
            OutputFormat::StreamJson => {
                if let Ok(json) = serde_json::to_string(event) {
                    println!("{}", json);
                }
            }
            OutputFormat::PlainText => match event {
                AgentEvent::TextDelta(text) => {
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                AgentEvent::ToolStart { name, input, .. } => {
                    if self.verbose {
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
                    } else if self.verbose {
                        println!("{}", format!("[tool: {name}] OK: {output}").green());
                    }
                }
                AgentEvent::TurnComplete { usage, .. } => {
                    println!(
                        "\n{}",
                        format!("[tokens: input={}, output={}]", usage.input_tokens, usage.output_tokens)
                            .bright_black()
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
                AgentEvent::ThinkingDelta(text) => {
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                AgentEvent::LogDelta { log, stream, .. } => {
                    if stream == "stderr" {
                        print!("{}", log.bright_red());
                    } else {
                        print!("{}", log.bright_black());
                    }
                    let _ = std::io::stdout().flush();
                }
                AgentEvent::Iteration { current, total } => {
                    if self.verbose {
                        println!("\n{}", format!("[iteration {}/{}]", current, total).bright_black());
                    }
                }
                AgentEvent::SystemLog(log) => {
                    if self.verbose {
                        println!("\n{}", format!("[system: {}]", log).bright_black());
                    }
                }
                AgentEvent::AssistantMessage { content } => {
                    for block in content {
                        if let nova_core::message::ContentBlock::Text { text } = block {
                            println!("\n{text}");
                        }
                    }
                }
                AgentEvent::AgentSwitched { agent_name, .. } => {
                    println!("\n{}", format!("[agent switched] {agent_name}").bright_black());
                }
            },
        }
    }

    fn print_error(&self, error: &dyn std::fmt::Display) {
        match self.format {
            OutputFormat::StreamJson => {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "Error",
                        "message": error.to_string()
                    })
                );
            }
            OutputFormat::PlainText => {
                eprintln!("\n{}", format!("[error] {}", error).red());
            }
        }
    }
}
