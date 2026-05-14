//! CLI for zero-nova library

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use custom_utils::args::workspace as resolve_workspace;
use custom_utils::logger::logger_feature;
use log::info;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::config::AppConfig;
use nova_agent::event::AgentEvent;
use nova_agent::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use nova_agent::mcp::client::McpClient;
use nova_agent::message::{ContentBlock, Message, Role};
use nova_agent::network::HttpClients;
use nova_agent::prompt::{EnvironmentSnapshot, SystemPromptBuilder, TrimmerConfig};
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use nova_agent::provider::LlmClient;
use nova_agent::skill::{SkillPackage, SkillRegistry, ToolPolicy};
use nova_agent::tool::builtin::task::{TaskStore, TaskStoreHandle};
use nova_agent::tool::{builtin::register_builtin_tools, ToolRegistry, UnavailableProjectDirService};
use nova_skill_loader::{load_single_skill, load_skills_from_dir, LoadedSkill, LoadedSkillPackage, LoadedToolPolicy};
use rustyline::history::FileHistory;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::ctrl_c;
use tokio::sync::mpsc;

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
    /// Agent id from config
    #[arg(long, global = true)]
    agent: Option<String>,
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

    let config = AppConfig::load_from_file(&config_path, workspace.clone())?;
    let root_agent = config.selected_agent(cli.agent.as_deref())?;
    let root_binding = config.resolve_agent_binding(root_agent)?;

    log::info!("Starting Nova CLI with : {:?}", config);
    let client = OpenAiCompatClient::from_registry_with_context_headers_enabled(
        config.providers.clone(),
        root_binding.provider_id.clone(),
        config.outbound_context_headers.enabled,
    );

    let env_snapshot = {
        let mut snapshot = EnvironmentSnapshot::collect(&config.config_dir, Some(&config.config_dir)).await;
        snapshot.model_id = Some(root_binding.model_config.model.clone());
        snapshot
    };

    let skill_dir = config.skills_dir();
    let mut loaded_skills = match load_skills_from_dir(&skill_dir) {
        Ok(skills) => skills,
        Err(e) => {
            if matches!(cli.output_format, OutputFormat::PlainText) {
                log::warn!("Failed to load skills from {:?}: {}", skill_dir, e);
            }
            Vec::new()
        }
    };
    if let Some(extra_skill_path) = &cli.include_skill {
        let path = Path::new(extra_skill_path);
        match load_single_skill(path) {
            Ok(Some(skill)) => loaded_skills.push(skill),
            Ok(None) => log::warn!("Included skill path {:?} did not contain a valid skill", path),
            Err(e) => log::error!("Failed to load included skill from {:?}: {}", path, e),
        }
    }
    let skill_registry_raw = SkillRegistry::from_packages(convert_loaded_skills(loaded_skills)).unwrap_or_else(|err| {
        log::warn!("Failed to initialize skill registry from loaded skills: {}", err);
        SkillRegistry::new()
    });

    let skill_prompt = skill_registry_raw.generate_contextual_prompt(None);
    let skill_registry = Arc::new(skill_registry_raw);

    let task_store = TaskStoreHandle::new(TaskStore::new());

    let tools = ToolRegistry::new();
    let http_clients = HttpClients::new()?;
    register_builtin_tools(
        &tools,
        &config,
        task_store.clone(),
        skill_registry.clone(),
        None,
        Arc::new(UnavailableProjectDirService::new(
            "ProjectManager is unavailable in CLI mode",
        )),
        &http_clients,
    );

    let prompt_builder = SystemPromptBuilder::new();
    let system_prompt_str = prompt_builder.with_tools(&tools).build();
    let final_system_prompt = format!("{}\n\n{}", system_prompt_str, skill_prompt);

    // Use config defaults instead of hardcoded values (synchronizes with nova-app bootstrap)
    let tool_timeout_secs = config.gateway.tool_timeout_secs.unwrap_or(300);
    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: root_binding.model_config.clone(),
        tool_timeout: Duration::from_secs(tool_timeout_secs),
        max_tokens: config.gateway.max_tokens,
        trimmer: TrimmerConfig {
            context_window: config.gateway.trimmer.context_window,
            output_reserve: config.gateway.trimmer.output_reserve,
            min_recent_messages: config.gateway.trimmer.min_recent_messages,
            enable_summary: false,
        },
        config_dir: config.config_dir.clone(),
        prompts_dir: config.prompts_dir(),
        project_context_file: config.project_context_file(),
        initial_env_snapshot: Some(env_snapshot),
        loop_guard: LoopGuardConfig {
            enabled: config.gateway.loop_guard.enabled,
            max_consecutive_duplicate_tool_calls: config.gateway.loop_guard.max_consecutive_duplicate_tool_calls,
            max_stalled_iterations: config.gateway.loop_guard.max_stalled_iterations,
            duplicate_read_mode: if config.gateway.loop_guard.duplicate_read_mode == "warn_only" {
                DuplicateReadMode::WarnOnly
            } else {
                DuplicateReadMode::WarnThenReject
            },
            iteration_trim_ratio: config.gateway.loop_guard.iteration_trim_ratio,
        },
        prompt_diagnostics: PromptDiagnosticsConfig {
            enabled: config.gateway.prompt_diagnostics.enabled,
            large_section_chars: config.gateway.prompt_diagnostics.large_section_chars,
            large_message_chars: config.gateway.prompt_diagnostics.large_message_chars,
            large_tool_result_chars: config.gateway.prompt_diagnostics.large_tool_result_chars,
        },
        tool_result_compaction: ToolResultCompactionConfig {
            enabled: config.gateway.tool_result_compaction.enabled,
            max_chars: config.gateway.tool_result_compaction.max_chars,
            head_chars: config.gateway.tool_result_compaction.head_chars,
            tail_chars: config.gateway.tool_result_compaction.tail_chars,
            disable_for_tools: config
                .gateway
                .tool_result_compaction
                .disable_for_tools
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
        },
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

fn convert_loaded_skills(loaded: Vec<LoadedSkill>) -> Vec<SkillPackage> {
    loaded
        .into_iter()
        .map(|skill| match skill {
            LoadedSkill::Package(package) => convert_package(package),
            LoadedSkill::Compat { package, .. } => convert_package(package),
        })
        .collect()
}

fn convert_package(package: LoadedSkillPackage) -> SkillPackage {
    SkillPackage {
        id: package.id,
        slug: package.slug,
        display_name: package.display_name,
        description: package.description,
        instructions: package.instructions,
        tool_policy: convert_tool_policy(package.tool_policy),
        sticky: package.sticky,
        aliases: package.aliases,
        examples: package.examples,
        source_path: package.source_path,
        compat_mode: package.compat_mode,
    }
}

fn convert_tool_policy(policy: LoadedToolPolicy) -> ToolPolicy {
    match policy {
        LoadedToolPolicy::InheritAll => ToolPolicy::InheritAll,
        LoadedToolPolicy::AllowList(tools) => ToolPolicy::AllowList(tools),
        LoadedToolPolicy::AllowListWithDeferred(tools) => ToolPolicy::AllowListWithDeferred(tools),
    }
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
        history.push(Message::new(
            Role::System,
            vec![ContentBlock::Text {
                text: system_prompt.to_string(),
            }],
            chrono::Utc::now().timestamp_millis(),
        ));
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
                print_tasks(agent).await;
                continue;
            }
            "/status" => {
                print_status(agent).await;
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
                    result = agent.run_turn(
                        &history,
                        input,
                        "cli-repl",
                        None,
                        agent.config.initial_env_snapshot.clone(),
                        tx.clone(),
                        None,
                    ) => {
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
        history.push(Message::new(
            Role::System,
            vec![ContentBlock::Text {
                text: system_prompt.to_string(),
            }],
            chrono::Utc::now().timestamp_millis(),
        ));
    }

    let result = agent
        .run_turn(
            &history,
            user_input,
            "cli-oneshot",
            None,
            agent.config.initial_env_snapshot.clone(),
            tx,
            None,
        )
        .await;
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
    let deferred = tools.deferred_definitions_snapshot();
    println!("  - Loaded tools: {}", loaded.len());
    println!("  - Deferred tools: {}", deferred.len());
}

/// Prints the list of available skills.
fn print_skills(agent: &AgentRuntime<impl LlmClient>) {
    if let Some(skill_registry) = &agent.skill_registry {
        println!("{}", "Available Skills:".bold());
        let candidates = skill_registry.all_candidates();
        if candidates.is_empty() {
            println!(
                "  (none loaded) Add a skill under `{}` with `SKILL.md` or `skill.toml`.",
                agent.config.config_dir.join("skills").display()
            );
            return;
        }
        for candidate in candidates {
            println!("- {} ({})", candidate.id, candidate.display_name);
        }
    } else {
        println!("{}", "No skill registry available.".yellow());
    }
}

/// Prints the task status.
async fn print_tasks(agent: &AgentRuntime<impl LlmClient>) {
    if let Some(task_store) = &agent.task_store {
        let tasks = snapshot_tasks(task_store).await;
        if tasks.is_empty() {
            println!("{}", "No tasks found.".blue());
        } else {
            println!("{}", "Tasks:".bold());
            for task in &tasks {
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
async fn print_status(agent: &AgentRuntime<impl LlmClient>) {
    println!("{}", "Overall Status:".bold());
    println!();

    println!("  Agent:");
    println!("    - Max iterations: 15");
    println!("    - Model: N/A");
    println!();

    if let Some(task_store) = &agent.task_store {
        println!("  Tasks: {} registered", snapshot_tasks(task_store).await.len());
    }

    if let Some(skill_registry) = &agent.skill_registry {
        println!("  Skills: {} available", skill_registry.all_candidates().len());
    }

    println!();
    println!("  Tool Capabilities:");
    println!("    - Always enabled tools: {}", agent.tools().tool_definitions().len());
    let deferred = agent.tools().deferred_definitions_snapshot();
    println!("    - Deferred tools: {}", deferred.len());
}

async fn snapshot_tasks(task_store: &TaskStoreHandle) -> Vec<nova_agent::tool::builtin::task::Task> {
    task_store.list_tasks().await
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
                    let summary = summarize_tool_start(name, input);
                    if self.verbose {
                        println!("\n{} {}", format!("[tool: {name}]").cyan(), summary.bright_black());
                        println!("{} {input:?}", "  input:".bright_black());
                    } else if summary.is_empty() {
                        println!("\n{}", format!("[tool: {name}]").cyan());
                    } else {
                        println!("\n{} {}", format!("[tool: {name}]").cyan(), summary.bright_black());
                    }
                }
                AgentEvent::ToolEnd {
                    name, output, is_error, ..
                } => {
                    if *is_error {
                        println!("{}", format!("[tool: {name}] ERROR: {output}").red());
                    } else if self.verbose {
                        println!("{}", format!("[tool: {name}] OK: {output}").green());
                    } else {
                        let summary = summarize_tool_end(name, output);
                        if !summary.is_empty() {
                            println!("{} {}", format!("[tool: {name}] OK").green(), summary.bright_black());
                        }
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

fn summarize_tool_start(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" => input
            .get("path")
            .and_then(|value| value.as_str())
            .map(|path| format!("path={}", path))
            .unwrap_or_default(),
        "Bash" => {
            let description = input.get("description").and_then(|value| value.as_str()).unwrap_or("");
            let command = input.get("command").and_then(|value| value.as_str()).unwrap_or("");
            if !description.is_empty() {
                format!("desc={}, cmd={}", description, truncate_inline(command, 80))
            } else if !command.is_empty() {
                format!("cmd={}", truncate_inline(command, 80))
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn summarize_tool_end(name: &str, output: &str) -> String {
    match name {
        "Read" => first_non_empty_line(output)
            .map(|line| truncate_inline(line, 100))
            .unwrap_or_default(),
        "Bash" => output
            .lines()
            .find(|line| line.starts_with("exit_code:"))
            .map(|line| line.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn truncate_inline(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::first_non_empty_line;
    use super::snapshot_tasks;
    use super::summarize_tool_start;
    use super::truncate_inline;
    use super::CliCommand;
    use nova_agent::tool::builtin::task::{TaskStore, TaskStoreHandle};
    use serde_json::json;

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

    #[tokio::test]
    async fn snapshot_tasks_can_run_inside_runtime() {
        let task_store = TaskStoreHandle::new(TaskStore::new());
        task_store
            .create_task(
                "status check".to_string(),
                "verify async snapshot path".to_string(),
                Some("checking status".to_string()),
                None,
                true,
            )
            .await;

        let tasks = snapshot_tasks(&task_store).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].subject, "status check");
    }

    #[test]
    fn summarize_bash_tool_start_prefers_description() {
        let input = json!({
            "description": "列出源码目录",
            "command": "Get-ChildItem src -Force"
        });
        let summary = summarize_tool_start("Bash", &input);
        assert!(summary.contains("desc=列出源码目录"));
        assert!(summary.contains("cmd=Get-ChildItem src -Force"));
    }

    #[test]
    fn truncate_inline_adds_ellipsis() {
        let result = truncate_inline("abcdefghijklmnopqrstuvwxyz", 5);
        assert_eq!(result, "abcde...");
    }

    #[test]
    fn first_non_empty_line_skips_blank_lines() {
        assert_eq!(first_non_empty_line("\n\nhello\nworld"), Some("hello"));
    }
}
