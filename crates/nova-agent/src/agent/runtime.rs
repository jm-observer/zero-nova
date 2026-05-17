use crate::event::AgentEvent;
use crate::loop_guard::LoopGuardConfig;
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::{
    ActiveSkillState, EnvironmentSnapshot, HistoryTrimmer, SideChannelInjector, SystemPromptBuilder, TrimmerConfig,
    TurnContext,
};
use crate::provider::types::{ToolDefinition, Usage};
use crate::provider::{LlmClient, ModelConfig};
use crate::skill::{CapabilityPolicy, SkillRegistry};
use crate::tool::builtin::task::TaskStoreHandle;
use crate::tool::Tool;
pub use crate::tool::ToolRegistry;
use anyhow::Result;
use serde::Serialize;
use serde_json::{self, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tool_exec::ExecuteTurnLoopRequest;
mod diagnostics;
mod guards;
mod tool_exec;

#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    pub messages: Vec<Message>,
    pub usage: Usage,
    pub provider_request_body: Option<Value>,
    pub provider_response_body: Option<Value>,
}

pub struct TurnRequest<'a> {
    pub history: &'a [Message],
    pub user_input: &'a str,
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub environment: Option<EnvironmentSnapshot>,
    pub event_tx: mpsc::Sender<AgentEvent>,
    pub cancellation_token: Option<CancellationToken>,
    pub model_config: &'a ModelConfig,
}

pub struct TurnWithContextRequest<'a> {
    pub ctx: TurnContext,
    pub message: Message,
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub environment: Option<EnvironmentSnapshot>,
    pub event_tx: mpsc::Sender<AgentEvent>,
    pub cancellation_token: Option<CancellationToken>,
    pub model_config: &'a ModelConfig,
}

/// Runtime for the zero-nova agent.
pub struct AgentRuntime {
    client: Box<dyn LlmClient>,
    tools: ToolRegistry,
    pub config: AgentConfig,
    pub task_store: Option<TaskStoreHandle>,
    pub skill_registry: Option<Arc<SkillRegistry>>,
    /// Session-level state: files that have been read across turns, used for Write pre-read enforcement.
    /// This is intentionally separate from per-turn duplicate-read convergence state.
    pub read_files: Arc<Mutex<HashSet<String>>>,
    /// 侧信道注入器（Phase 3 新增）
    pub side_channel_injector: Option<SideChannelInjector>,
}

/// Configuration for the zero-nova agent.
pub struct AgentConfig {
    pub max_iterations: usize,
    pub model_config: ModelConfig,
    pub tool_timeout: Duration,
    /// 最大 token 限制
    pub max_tokens: usize,
    /// 历史裁剪配置
    pub trimmer: TrimmerConfig,
    /// 配置目录路径
    pub config_dir: PathBuf,
    /// 提示词目录 (AppConfig::prompts_dir)
    pub prompts_dir: PathBuf,
    /// 项目上下文文件路径
    pub project_context_file: Option<PathBuf>,
    /// 启动时采集的环境快照
    pub initial_env_snapshot: Option<EnvironmentSnapshot>,
    /// 循环保护配置
    pub loop_guard: LoopGuardConfig,
    /// Prompt 体量诊断配置
    pub prompt_diagnostics: PromptDiagnosticsConfig,
    /// Tool result 历史压缩配置
    pub tool_result_compaction: ToolResultCompactionConfig,
}

#[derive(Debug, Clone)]
pub struct PromptDiagnosticsConfig {
    pub enabled: bool,
    pub large_section_chars: usize,
    pub large_message_chars: usize,
    pub large_tool_result_chars: usize,
}

#[derive(Debug, Clone)]
pub struct ToolResultCompactionConfig {
    pub enabled: bool,
    pub max_chars: usize,
    pub head_chars: usize,
    pub tail_chars: usize,
    pub disable_for_tools: HashSet<String>,
}

impl AgentRuntime {
    /// Creates a new `AgentRuntime` instance.
    pub fn new(client: impl LlmClient + 'static, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self {
            client: Box::new(client),
            tools,
            config,
            task_store: None,
            skill_registry: None,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            side_channel_injector: None,
        }
    }

    /// Sets the tool registry for this runtime.
    pub fn set_tools(&mut self, tools: ToolRegistry) {
        self.tools = tools;
    }

    /// Registers a new tool with the registry.
    pub async fn register_tool(&self, tool: Box<dyn Tool>) {
        self.tools.register(tool).await;
    }

    /// Returns a reference to the tool registry.
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// 设置侧信道注入器。
    pub fn set_side_channel_injector(&mut self, injector: SideChannelInjector) {
        self.side_channel_injector = Some(injector);
    }

    /// Executes a single turn of the agent, handling LLM streaming and tool execution.
    pub async fn run_turn(
        &self,
        history: &[Message],
        user_input: &str,
        session_id: &str,
        agent_id: &str,
        environment: Option<EnvironmentSnapshot>,
        event_tx: mpsc::Sender<AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<TurnResult> {
        self.run_turn_with_model_config(TurnRequest {
            history,
            user_input,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config: &self.config.model_config,
        })
        .await
    }

    pub async fn run_turn_with_model_config(&self, req: TurnRequest<'_>) -> Result<TurnResult> {
        let TurnRequest {
            history,
            user_input,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config,
        } = req;
        let turn_ctx = self
            .prepare_turn(user_input, Arc::new(history.to_vec()), String::new(), session_id)
            .await?;
        let user_message = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: user_input.to_string(),
            }],
            chrono::Utc::now().timestamp_millis(),
        );

        self.run_turn_with_context_and_model_config(TurnWithContextRequest {
            ctx: turn_ctx,
            message: user_message,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config,
        })
        .await
    }

    // -----------------------------------------------------------------------
    //  Plan 2 — Turn 前准备（Turn before run）
    // -----------------------------------------------------------------------

    /// 准备 turn 上下文：决定 active skill、生成 system prompt sections、
    /// 过滤工具定义、裁剪历史、构造 `TurnContext`。
    ///
    /// `system_prompt` 由调用方预先构建，runtime 不感知外部文件加载。
    pub async fn prepare_turn(
        &self,
        input: &str,
        current_history: Arc<Vec<Message>>,
        system_prompt: String,
        session_id: &str,
    ) -> Result<TurnContext> {
        // 1. 决定 active skill
        let active_skill = self.decide_active_skill(input, &current_history)?;

        // 2. 根据 active skill 生成 capability policy（仅保留元数据，不裁剪工具）
        let capability_policy = if let Some(ref as2) = active_skill {
            if let Some(ref sr) = self.skill_registry {
                sr.policy_from_skill(&as2.skill_id)
            } else {
                CapabilityPolicy::default()
            }
        } else {
            CapabilityPolicy::default()
        };

        // 3. 获取统一工具视图（loaded + deferred）
        let turn_view = self.tools.get_turn_view(session_id, true, true, true).await;
        let tool_definitions: Vec<_> = turn_view
            .loaded
            .iter()
            .map(|d| ToolDefinition {
                name: d.name.clone(),
                description: d.description.clone(),
                input_schema: d.input_schema.clone(),
            })
            .collect();

        // 将 deferred tool 摘要追加到系统提示词
        let system_prompt = if turn_view.deferred.is_empty() {
            system_prompt
        } else {
            let mut deferred_section = String::from(
                "\n\n## Deferred Tools\n\nThe following tools are available but not loaded. Use `ToolSearch` to search and activate them:\n",
            );
            for d in &turn_view.deferred {
                deferred_section.push_str(&format!("- `{}`: {}\n", d.name, d.description));
            }
            format!("{}{}", system_prompt, deferred_section)
        };

        // 4. 裁剪历史（如果 active skill 切换了则裁剪）
        let prompt_diag_builder = SystemPromptBuilder::new().base_section(system_prompt.clone());
        self.log_prompt_diagnostics(&prompt_diag_builder, &tool_definitions);
        let history = self.trim_history(&current_history, &system_prompt, &active_skill)?;
        self.log_history_diagnostics(history.as_ref());

        // 5. 构造 TurnContext
        let visible_tool_names: std::sync::Arc<std::collections::HashSet<String>> =
            std::sync::Arc::new(tool_definitions.iter().map(|t| t.name.clone()).collect());
        Ok(TurnContext {
            system_prompt,
            tool_definitions,
            visible_tool_names,
            history,
            active_skill,
            capability_policy,
            skill_tool_enabled: true,
            max_tokens: self.config.max_tokens,
            iteration_budget: self.config.max_iterations,
        })
    }

    /// 预解析本轮输入会激活的 skill id，供调用方在构建 system prompt 前注入 active skill 内容。
    pub fn resolve_active_skill_id(&self, input: &str, current_history: &[Message]) -> Result<Option<String>> {
        Ok(self
            .decide_active_skill(input, current_history)?
            .map(|active_skill| active_skill.skill_id))
    }

    /// 运行 turn 并使用 `TurnContext`。
    ///
    /// 接收已经通过 `prepare_turn()` 准备好的上下文，
    /// CLI / app / gateway 共用同一套准备逻辑。
    ///
    /// Phase 3 重写：补全工具执行逻辑和 usage 统计。
    pub async fn run_turn_with_context(
        &self,
        ctx: TurnContext,
        message: Message,
        session_id: &str,
        agent_id: &str,
        environment: Option<EnvironmentSnapshot>,
        event_tx: mpsc::Sender<AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<TurnResult> {
        self.run_turn_with_context_and_model_config(TurnWithContextRequest {
            ctx,
            message,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config: &self.config.model_config,
        })
        .await
    }

    pub async fn run_turn_with_context_and_model_config(&self, req: TurnWithContextRequest<'_>) -> Result<TurnResult> {
        let TurnWithContextRequest {
            ctx,
            message,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config,
        } = req;
        // 尝试直接移动所有权；失败则 clone 是预期行为（性能优化，语义等价）。
        let mut all_messages = Arc::try_unwrap(ctx.history)
            .unwrap_or_else(|h| (*h).clone())
            .into_iter()
            .collect::<Vec<_>>();

        // 注入最新的系统提示词
        if let Some(first) = all_messages.get_mut(0) {
            if first.role == Role::System {
                first.content = vec![ContentBlock::Text {
                    text: ctx.system_prompt.clone(),
                }];
            } else {
                all_messages.insert(
                    0,
                    Message::new(
                        Role::System,
                        vec![ContentBlock::Text {
                            text: ctx.system_prompt.clone(),
                        }],
                        chrono::Utc::now().timestamp_millis(),
                    ),
                );
            }
        } else {
            all_messages.push(Message::new(
                Role::System,
                vec![ContentBlock::Text {
                    text: ctx.system_prompt.clone(),
                }],
                chrono::Utc::now().timestamp_millis(),
            ));
        }

        all_messages.push(message);

        self.execute_turn_loop(ExecuteTurnLoopRequest {
            all_messages,
            tool_definitions: &ctx.tool_definitions,
            visible_tool_names: ctx.visible_tool_names.clone(),
            iteration_budget: ctx.iteration_budget,
            session_id,
            agent_id,
            environment,
            event_tx,
            cancellation_token,
            model_config,
        })
        .await
    }

    /// 决定 active skill 路由（阶段一：规则路由）。
    fn decide_active_skill(&self, input: &str, _current_history: &[Message]) -> Result<Option<ActiveSkillState>> {
        if let Some(ref sr) = self.skill_registry {
            // 检查显式退出信号
            if sr.is_exit_signal(input) {
                return Ok(None);
            }

            // Mode A: /skill-name 模式
            if let Some(matched_id) = sr.match_skill_by_input(input) {
                return Ok(Some(ActiveSkillState::new(matched_id)));
            }
        }

        // 阶段一：返回 None（后续添加 Sticky + LLM 路由）
        Ok(None)
    }

    /// 裁剪历史（如果 active skill 切换了则裁剪）。
    ///
    /// Phase 3：接入 `HistoryTrimmer` 进行 token 预算感知的裁剪。
    fn trim_history(
        &self,
        current_history: &Arc<Vec<Message>>,
        system_prompt: &str,
        active_skill: &Option<ActiveSkillState>,
    ) -> Result<Arc<Vec<Message>>> {
        if active_skill.is_none() {
            return Ok(current_history.clone());
        }

        let trimmer = HistoryTrimmer::new(self.config.trimmer.clone());
        let result = trimmer.trim(current_history, system_prompt);

        if result.was_trimmed {
            log::info!(
                "History trimmed: removed {} messages to fit context window",
                result.removed_count
            );
        }

        Ok(Arc::new(result.messages))
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
    use crate::loop_guard::LoopGuardConfig;
    use crate::message::Message;
    use crate::prompt::TrimmerConfig;
    use crate::provider::types::{ProviderRequestContext, ToolDefinition};
    use crate::provider::{LlmClient, ModelConfig, StreamReceiver};
    use crate::skill::{SkillPackage, SkillRegistry, ToolPolicy};
    use crate::tool::ToolRegistry;
    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    struct NoopClient;

    #[async_trait]
    impl LlmClient for NoopClient {
        async fn stream(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _config: &ModelConfig,
            _request_context: &ProviderRequestContext,
        ) -> Result<Box<dyn StreamReceiver>> {
            unreachable!("not used in compaction unit tests")
        }
    }

    fn build_runtime(disable_for_tools: HashSet<String>) -> AgentRuntime {
        AgentRuntime::new(
            NoopClient,
            ToolRegistry::new(),
            AgentConfig {
                max_iterations: 1,
                model_config: ModelConfig {
                    provider: None,
                    model: "test".to_string(),
                    max_tokens: 128,
                    temperature: None,
                    top_p: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                    max_tokens_field: "completion".to_string(),
                    extra_body: None,
                },
                tool_timeout: Duration::from_secs(1),
                max_tokens: 128,
                trimmer: TrimmerConfig::default(),
                config_dir: std::path::PathBuf::new(),
                prompts_dir: std::path::PathBuf::new(),
                project_context_file: None,
                initial_env_snapshot: None,
                loop_guard: LoopGuardConfig::default(),
                prompt_diagnostics: PromptDiagnosticsConfig {
                    enabled: false,
                    large_section_chars: 8_000,
                    large_message_chars: 12_000,
                    large_tool_result_chars: 8_000,
                },
                tool_result_compaction: ToolResultCompactionConfig {
                    enabled: true,
                    max_chars: 20,
                    head_chars: 6,
                    tail_chars: 6,
                    disable_for_tools,
                },
            },
        )
    }

    #[test]
    fn compact_tool_output_keeps_short_output() {
        let runtime = build_runtime(HashSet::new());
        let output = runtime.compact_tool_output("Read", false, "short");
        assert_eq!(output, "short");
    }

    #[test]
    fn compact_tool_output_compacts_long_output() {
        let runtime = build_runtime(HashSet::new());
        let output = runtime.compact_tool_output("Read", false, "0123456789abcdefghijklmnopqrstuvwxyz");
        assert!(output.contains("[Tool output compacted]"));
        assert!(output.contains("Tool: Read"));
        assert!(output.contains("--- head ---"));
        assert!(output.contains("--- tail ---"));
    }

    #[test]
    fn compact_tool_output_respects_tool_disable_list() {
        let mut disabled = HashSet::new();
        disabled.insert("read".to_string());
        let runtime = build_runtime(disabled);
        let raw = "0123456789abcdefghijklmnopqrstuvwxyz";
        let output = runtime.compact_tool_output("Read", false, raw);
        assert_eq!(output, raw);
    }

    #[test]
    fn compact_tool_output_handles_utf8_boundaries() {
        let runtime = build_runtime(HashSet::new());
        let raw = "你好🙂世界🙂你好🙂世界🙂你好🙂世界🙂你好🙂世界🙂";
        let output = runtime.compact_tool_output("Read", false, raw);
        assert!(output.contains("[Tool output compacted]"));
        assert!(std::str::from_utf8(output.as_bytes()).is_ok());
    }

    #[test]
    fn resolve_active_skill_id_matches_input_before_prompt_build() {
        let mut runtime = build_runtime(HashSet::new());
        let mut registry = SkillRegistry::new();
        registry
            .extend_packages(vec![SkillPackage {
                id: "demo".to_string(),
                slug: "demo".to_string(),
                display_name: "Demo".to_string(),
                description: "demo skill".to_string(),
                instructions: "demo instructions".to_string(),
                tool_policy: ToolPolicy::InheritAll,
                sticky: false,
                aliases: vec!["d".to_string()],
                examples: vec![],
                source_path: PathBuf::from("demo"),
                compat_mode: false,
            }])
            .unwrap();
        runtime.skill_registry = Some(Arc::new(registry));

        let active_skill = runtime.resolve_active_skill_id("/d run this", &[]).unwrap();

        assert_eq!(active_skill.as_deref(), Some("demo"));
    }
}
