//! CLI for zero-nova library

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use custom_utils::args::workspace as resolve_workspace;
use custom_utils::logger::logger_feature;
use log::info;
use nova_agent::agent::{AgentConfig, AgentRuntime};
use nova_agent::config::{AppConfig, OriginAppConfig};
use nova_agent::event::AgentEvent;
use nova_agent::mcp::client::McpClient;
use nova_agent::message::{ContentBlock, Message, Role};
use nova_agent::prompt::{EnvironmentSnapshot, SystemPromptBuilder, TrimmerConfig};
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use nova_agent::provider::LlmClient;
use nova_agent::skill::SkillRegistry;
use nova_agent::tool::builtin::task::TaskStore;
use nova_agent::tool::{builtin::register_builtin_tools, ToolRegistry};
use rustyline::history::FileHistory;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::signal::ctrl_c;
use tokio::sync::{mpsc, Mutex};

/// CLI 调试命令枚举（Plan 4）。
#[derive(Debug, Clone)]
pub enum CliCommand {
    /// 列出当前可用 skill 与 active skill
    Skills,
    /// 手动激活某个 skill，便于调试
    SkillActivate(String),
    /// 退出当前 skill
    SkillExit,
    /// 查看当前轮实际组装的 prompt sections
    PromptSections,
    /// 查看当前 session 的 task 状态
    Tasks,
    /// 查看当前轮次可见工具视图
    Tools,
    /// 显示整体状态（skill/agent/tool-policy）
    Status,
    /// 普通用户消息
    Message(String),
}

impl CliCommand {
    /// 解析用户输入为 CliCommand。
    pub fn parse(input: &str) -> CliCommand {
        if input.starts_with('/') {
            return match input.split_whitespace().next() {
                Some("/skills") => CliCommand::Skills,
                Some("/skill") => CliCommand::SkillActivate(input[6..].trim().to_string()),
                Some("/exit-skill") => CliCommand::SkillExit,
                Some("/prompt-sections") => CliCommand::PromptSections,
                Some("/tasks") => CliCommand::Tasks,
                Some("/tools") => CliCommand::Tools,
                Some("/status") => CliCommand::Status,
                _ => CliCommand::Message(input.to_string()),
            };
        }
        CliCommand::Message(input.to_string())
    }
}

impl std::fmt::Display for CliCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliCommand::Skills => write!(f, "/skills"),
            CliCommand::SkillActivate(id) => write!(f, "/skill {}", id),
            CliCommand::SkillExit => write!(f, "/exit-skill"),
            CliCommand::PromptSections => write!(f, "/prompt-sections"),
            CliCommand::Tasks => write!(f, "/tasks"),
            CliCommand::Tools => write!(f, "/tools"),
            CliCommand::Status => write!(f, "/status"),
            CliCommand::Message(msg) => write!(f, "{}", msg),
        }
    }
}

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
    let _ = logger_feature("nova_cli", "debug,rustyline=info", log::LevelFilter::Info, false).build();

    let workspace = resolve_workspace(&cli.workspace, ".nova")?;
    info!("workspace {}", workspace.display());
    let config_path = workspace.join("config.toml");

    let mut config = OriginAppConfig::load_from_file(&config_path)?;

    if let Some(model) = &cli.model {
        config.llm.model_config.model = model.to_string();
    }
    if let Some(base_url) = &cli.base_url {
        config.llm.base_url = base_url.to_string();
    }

    let config = AppConfig::from_origin(config, workspace.clone());

    log::info!("Starting Nova CLI with : {:?}", config);
    let client = OpenAiCompatClient::new(config.llm.api_key.clone(), config.llm.base_url.clone());

    let env_snapshot = {
        let mut snapshot = EnvironmentSnapshot::collect().await;
        snapshot.model_id = Some(config.llm.model_config.model.clone());
        snapshot
    };

    let mut skill_registry_raw = SkillRegistry::new();
    let skill_dir = config.skills_dir();
    if let Err(e) = skill_registry_raw.load_from_dir(&skill_dir) {
        if matches!(cli.output_format, OutputFormat::PlainText) {
            log::warn!("Failed to load skills from {:?}: {}", skill_dir, e);
        }
    }
    if let Some(extra_skill_path) = &cli.include_skill {
        let path = Path::new(extra_skill_path);
        if let Err(e) = skill_registry_raw.load_single_skill(path) {
            log::error!("Failed to load included skill from {:?}: {}", path, e);
        }
    }

    let skill_prompt = skill_registry_raw.generate_contextual_prompt(None);
    let skill_registry = Arc::new(skill_registry_raw);

    let task_store = Arc::new(Mutex::new(TaskStore::new()));

    let tools = ToolRegistry::new();
    register_builtin_tools(&tools, &config, task_store.clone(), skill_registry.clone(), None);

    let prompt_builder = SystemPromptBuilder::new();
    let system_prompt_str = prompt_builder.with_tools(&tools).build();
    let final_system_prompt = format!("{}\n\n{}", system_prompt_str, skill_prompt);

    // Use config defaults instead of hardcoded values (synchronizes with nova-app bootstrap)
    let tool_timeout_secs = config.gateway.tool_timeout_secs.unwrap_or(300);
    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: config.llm.model_config.clone(),
        tool_timeout: Duration::from_secs(tool_timeout_secs),
        max_tokens: config.gateway.max_tokens,
        use_turn_context: config.gateway.use_turn_context,
        trimmer: TrimmerConfig {
            context_window: config.gateway.trimmer.context_window,
            output_reserve: config.gateway.trimmer.output_reserve,
            min_recent_messages: config.gateway.trimmer.min_recent_messages,
            enable_summary: false,
        },
        workspace: config.workspace.clone(),
        prompts_dir: config.prompts_dir(),
        project_context_file: config.project_context_file(),
        initial_env_snapshot: Some(env_snapshot),
    };

    let mut agent = AgentRuntime::new(client, tools, agent_config);
    agent.task_store = Some(task_store);
    agent.skill_registry = Some(skill_registry);

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

    if !system_prompt.is_empty() {
        history.push(Message {
            role: Role::System,
            content: vec![ContentBlock::Text {
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
                println!("  /quit       - Exit the CLI");
                println!("  /help       - Show this help message");
                println!("  /tools      - List all registered tools");
                println!("  /skills     - List available skills");
                println!("  /skill <id> - Activate a specific skill");
                println!("  /exit-skill - Exit current skill");
                println!("  /tasks      - Show task status");
                println!("  /status     - Show overall status");
                println!("  /prompt     - Show current system prompt");
                println!("  /clear      - Clear conversation history (keeps system prompt)");
                println!("  /history    - Show conversation history stats");
                println!("  /prompt-sections - Show prompt sections");
                continue;
            }
            "/tools" => {
                print_tools(agent);
                continue;
            }
            "/skills" => {
                print_skills(agent);
                continue;
            }
            "/tasks" => {
                print_tasks(agent);
                continue;
            }
            "/status" => {
                print_status(agent);
                continue;
            }
            "/exit-skill" => {
                println!("{}", "Exited skill (debug mode)".yellow());
                continue;
            }
            "/skill" => {
                println!("{}", "Skill activate command received".cyan());
                continue;
            }
            "/prompt-sections" => {
                println!("{}", "Prompt sections debug info".blue());
                continue;
            }
            "/clear" => {
                // Keep the first system message if it exists
                let system_msg = history.first().cloned().filter(|m| m.role == Role::System);
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
                if let Some(msg) = history.first().filter(|m| m.role == Role::System) {
                    for block in &msg.content {
                        if let ContentBlock::Text { text } = block {
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
                    _ = ctrl_c() => {
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
            role: Role::System,
            content: vec![ContentBlock::Text {
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
    let tools = agent.tools();
    println!("{}", "Registered Tools:".bold());
    for def in tools.tool_definitions() {
        println!("- {}: {}", def.name, def.description);
    }
    println!();
    println!("{}", "Turn Tool View:".bold());
    println!("  - Tool Search: {}", if true { "enabled" } else { "disabled" });
    let loaded = tools.loaded_definitions();
    let deferred = tools.deferred_definitions();
    println!("  - Loaded tools: {}", loaded.len());
    println!("  - Deferred tools: {}", deferred.len());
}

/// Prints the list of available skills.
fn print_skills(agent: &AgentRuntime<impl LlmClient>) {
    if let Some(skill_registry) = &agent.skill_registry {
        println!("{}", "Available Skills:".bold());
        for candidate in skill_registry.all_candidates() {
            println!("- {} ({})", candidate.id, candidate.display_name);
        }
    } else {
        println!("{}", "No skill registry available.".yellow());
    }
}

/// Prints the task status.
fn print_tasks(agent: &AgentRuntime<impl LlmClient>) {
    if let Some(task_store) = &agent.task_store {
        let store = Handle::current().block_on(async { task_store.lock().await });
        let tasks = store.list();
        if tasks.is_empty() {
            println!("{}", "No tasks found.".blue());
        } else {
            println!("{}", "Tasks:".bold());
            for task in tasks {
                let is_main = task.is_main_task;
                let main_marker = if is_main { "*" } else { " " };
                println!(
                    "  {} [{}] {} - {:?}: {}",
                    main_marker,
                    task.id,
                    task.subject,
                    task.status,
                    task.active_form.as_deref().unwrap_or("N/A")
                );
            }
        }
    } else {
        println!("{}", "No task store available.".yellow());
    }
}

/// Prints the overall status (skill/agent/tool-policy).
fn print_status(agent: &AgentRuntime<impl LlmClient>) {
    println!("{}", "Overall Status:".bold());
    println!();

    println!("  Agent:");
    println!("    - Max iterations: 15");
    println!("    - Model: N/A");
    println!();

    if let Some(task_store) = &agent.task_store {
        let store = Handle::current().block_on(async { task_store.lock().await });
        println!("  Tasks: {} registered", store.list().len());
    }

    if let Some(skill_registry) = &agent.skill_registry {
        println!("  Skills: {} available", skill_registry.all_candidates().len());
    }

    println!();
    println!("  Tool Capabilities:");
    println!("    - Always enabled tools: {}", agent.tools().tool_definitions().len());
    let deferred = agent.tools().deferred_definitions();
    println!("    - Deferred tools: {}", deferred.len());
}

/// Tests the MCP server by invoking the first tool.
async fn test_mcp(cmd: &[String]) -> Result<()> {
    if cmd.is_empty() {
        bail!("No command provided for MCP test");
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
                        if let ContentBlock::Text { text } = block {
                            println!("\n{text}");
                        }
                    }
                }
                AgentEvent::AgentSwitched { agent_name, .. } => {
                    println!("\n{}", format!("[agent switched] {agent_name}").bright_black());
                }
                AgentEvent::TaskCreated { id, subject } => {
                    println!("\n{}", format!("[task created] {id}: {subject}").bright_cyan());
                }
                AgentEvent::TaskStatusChanged { id, status, .. } => {
                    println!("\n{}", format!("[task {id}] status -> {status}").bright_cyan());
                }
                AgentEvent::BackgroundTaskComplete { name, .. } => {
                    println!("\n{}", format!("[bg task complete] {name}").bright_green());
                }
                AgentEvent::SkillLoaded { skill_name } => {
                    println!("\n{}", format!("[skill loaded] {skill_name}").bright_purple());
                }
                AgentEvent::SkillActivated { skill_name, .. } => {
                    println!("\n{}", format!("[skill activated] {skill_name}").bright_green());
                }
                AgentEvent::SkillSwitched { to_skill, .. } => {
                    println!("\n{}", format!("[skill switched] -> {to_skill}").bright_magenta());
                }
                AgentEvent::SkillExited { skill_id, .. } => {
                    println!("\n{}", format!("[skill exited] {skill_id}").yellow());
                }
                AgentEvent::SkillRouteEvaluated { .. } => {
                    // 该事件仅用于内部调试，CLI 不重复打印以避免输出噪音。
                }
                AgentEvent::ToolUnlocked { tool_name } => {
                    println!("\n{}", format!("[tool unlocked] {tool_name}").bright_blue());
                }
                AgentEvent::SkillInvocation { skill_name, level, .. } => {
                    println!(
                        "\n{}",
                        format!("[skill invoked:{:?}] {skill_name}", level).bright_cyan()
                    );
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

#[cfg(test)]
mod tests {
    use super::CliCommand;

    #[test]
    fn test_parse_skills_command() {
        assert!(matches!(CliCommand::parse("/skills"), CliCommand::Skills));
    }

    #[test]
    fn test_parse_skill_activate_with_id() {
        let cmd = CliCommand::parse("/skill my-skill");
        assert!(matches!(cmd, CliCommand::SkillActivate(id) if id == "my-skill"));
    }

    #[test]
    fn test_parse_skill_activate_empty_id() {
        let cmd = CliCommand::parse("/skill");
        assert!(matches!(cmd, CliCommand::SkillActivate(id) if id.is_empty()));
    }

    #[test]
    fn test_parse_exit_skill_command() {
        assert!(matches!(CliCommand::parse("/exit-skill"), CliCommand::SkillExit));
    }

    #[test]
    fn test_parse_prompt_sections_command() {
        assert!(matches!(
            CliCommand::parse("/prompt-sections"),
            CliCommand::PromptSections
        ));
    }

    #[test]
    fn test_parse_tasks_command() {
        assert!(matches!(CliCommand::parse("/tasks"), CliCommand::Tasks));
    }

    #[test]
    fn test_parse_tools_command() {
        assert!(matches!(CliCommand::parse("/tools"), CliCommand::Tools));
    }

    #[test]
    fn test_parse_status_command() {
        assert!(matches!(CliCommand::parse("/status"), CliCommand::Status));
    }

    #[test]
    fn test_parse_regular_message() {
        let cmd = CliCommand::parse("Hello, world!");
        assert!(matches!(cmd, CliCommand::Message(msg) if msg == "Hello, world!"));
    }

    #[test]
    fn test_parse_regular_message_without_slash() {
        let cmd = CliCommand::parse("write a file");
        assert!(matches!(cmd, CliCommand::Message(msg) if msg == "write a file"));
    }

    #[test]
    fn test_parse_unknown_slash_command_as_message() {
        let cmd = CliCommand::parse("/unknown");
        assert!(matches!(cmd, CliCommand::Message(msg) if msg == "/unknown"));
    }

    #[test]
    fn test_clisimple_command_display() {
        assert_eq!(format!("{}", CliCommand::Skills), "/skills");
        assert_eq!(format!("{}", CliCommand::SkillExit), "/exit-skill");
        assert_eq!(format!("{}", CliCommand::Tasks), "/tasks");
    }

    #[test]
    fn test_clicommand_display_with_target() {
        let cmd = format!("{}", CliCommand::SkillActivate("test-skill".to_string()));
        assert_eq!(cmd, "/skill test-skill");
    }
}
