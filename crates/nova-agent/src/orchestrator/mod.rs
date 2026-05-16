pub mod planner;
pub mod reviewer;
pub mod scheduler;

use crate::event::AgentEvent;
use crate::tool::ToolContext;
use anyhow::Result;
use async_trait::async_trait;
use nova_protocol::orchestration::{
    AgentSummary, OrchestrationCompleteArgs, OrchestrationPlanEvent, OrchestrationReviewStartArgs, StageCompleteArgs,
    StageSummary, SubAgentCompleteArgs, SubAgentSpawnArgs,
};
use planner::OrchestrationPlan;
use reviewer::{build_review_prompt, parse_review_result, ReviewResult};
use scheduler::{execute_stage, SubAgentResult, SubAgentStatus};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct SubAgentRequest {
    pub prompt: String,
    pub description: String,
    pub agent_selection: Option<String>,
    pub agent_id: Option<String>,
    pub plan_id: Option<String>,
    pub stage_id: Option<String>,
    pub output_format: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SubAgentOutput {
    pub output: String,
    pub duration_ms: u128,
    pub warnings: Vec<String>,
}

#[async_trait]
pub trait SubAgentExecutor: Send + Sync {
    async fn execute_agent(&self, request: SubAgentRequest, context: Option<ToolContext>) -> Result<SubAgentOutput>;

    fn catalog_agent_ids(&self) -> HashSet<String>;

    fn default_agent_id(&self) -> String;
}

pub struct OrchestratorEngine {
    executor: Arc<dyn SubAgentExecutor>,
    event_tx: mpsc::Sender<AgentEvent>,
    tool_context: Option<ToolContext>,
    /// Catalog 中已注册的 agent ID 集合（用于验证 LLM 的 agent 选择）。
    catalog_agent_ids: Arc<HashSet<String>>,
    /// 默认 agent ID（用于 fallback）。
    default_agent_id: String,
}

pub struct ExecutionOutcome {
    pub plan: OrchestrationPlan,
    pub results: HashMap<String, SubAgentResult>,
    pub review: Option<ReviewResult>,
}

enum StageFailure {
    StageFailed { stage_id: String },
    BlockedByDependency { stage_id: String, dependency: String },
}

impl StageFailure {
    fn summary(&self) -> String {
        match self {
            Self::StageFailed { stage_id } => {
                format!("Orchestration stopped after stage '{}' failed.", stage_id)
            }
            Self::BlockedByDependency { stage_id, dependency } => {
                format!("Stage '{}' blocked by dependency '{}'.", stage_id, dependency)
            }
        }
    }
}

impl OrchestratorEngine {
    pub fn new(
        executor: Arc<dyn SubAgentExecutor>,
        event_tx: mpsc::Sender<AgentEvent>,
        tool_context: Option<ToolContext>,
    ) -> Self {
        let catalog_agent_ids = Arc::new(executor.catalog_agent_ids());
        let default_agent_id = executor.default_agent_id();

        Self {
            executor,
            event_tx,
            tool_context,
            catalog_agent_ids,
            default_agent_id,
        }
    }

    /// 验证 agent 选择是否在 catalog 中。
    /// 如果不在 catalog 中，返回 warning 消息并 fallback 到默认 agent。
    pub fn validate_agent_selection(&self, selection: &str) -> (String, Option<String>) {
        if self.catalog_agent_ids.contains(selection) {
            (selection.to_string(), None)
        } else {
            let warning = format!(
                "Agent selection '{}' not found in catalog, falling back to '{}'",
                selection, self.default_agent_id
            );
            log::warn!("[OrchestratorEngine] {}", warning);
            (self.default_agent_id.clone(), Some(warning))
        }
    }

    /// 获取 review 阶段应使用的 agent ID。
    /// 优先使用 catalog 中的默认 agent，如果 catalog 中没有则 fallback 到 primary agent。
    pub fn review_agent_id(&self) -> String {
        // 检查 catalog 中是否有 "reviewer" agent
        if self.catalog_agent_ids.contains("reviewer") {
            "reviewer".to_string()
        } else {
            // 使用默认 agent
            self.default_agent_id.clone()
        }
    }

    pub async fn execute_plan(
        &self,
        plan: OrchestrationPlan,
        cancellation_token: CancellationToken,
    ) -> Result<ExecutionOutcome> {
        let mut results = HashMap::<String, SubAgentResult>::new();
        let mut stage_success = HashMap::<String, bool>::new();
        let max_retries = plan.max_retries.unwrap_or(1);

        log::info!(
            "[OrchestratorEngine] Parsed plan_id={} stage_count={} skip_review={} max_retries={} description={}",
            plan.plan_id,
            plan.stages.len(),
            plan.skip_review,
            max_retries,
            plan.description
        );

        self.emit_plan_event(&plan).await;

        let failure_summary = self
            .execute_all_stages(&plan, &cancellation_token, &mut results, &mut stage_success)
            .await?;

        if cancellation_token.is_cancelled() {
            log::warn!(
                "[OrchestratorEngine] Orchestration cancelled plan_id={} result_count={}",
                plan.plan_id,
                results.len()
            );
            self.emit_complete(&plan.plan_id, false, "Orchestration cancelled.")
                .await;
            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        if results.is_empty() {
            log::info!("[OrchestratorEngine] No stages scheduled for plan_id={}", plan.plan_id);
            self.emit_complete(&plan.plan_id, true, "No stages were scheduled.")
                .await;
            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        if let Some(failure) = &failure_summary {
            self.emit_complete(&plan.plan_id, false, &failure.summary()).await;
            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        if plan.skip_review {
            log::info!(
                "[OrchestratorEngine] Skipping review (skip_review=true) plan_id={}",
                plan.plan_id
            );
            self.emit_complete(&plan.plan_id, true, "All stages completed (review skipped).")
                .await;
            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        let review = self
            .run_review_with_retry(&plan, &mut results, &cancellation_token, max_retries)
            .await?;

        self.emit_complete(&plan.plan_id, review.success, &review.summary).await;

        log::info!(
            "[OrchestratorEngine] Orchestration complete plan_id={} overall_success={} agent_count={}",
            plan.plan_id,
            review.success,
            results.len()
        );

        Ok(ExecutionOutcome {
            plan,
            results,
            review: Some(review),
        })
    }

    async fn execute_all_stages(
        &self,
        plan: &OrchestrationPlan,
        cancellation_token: &CancellationToken,
        results: &mut HashMap<String, SubAgentResult>,
        stage_success: &mut HashMap<String, bool>,
    ) -> Result<Option<StageFailure>> {
        let mut failure: Option<StageFailure> = None;

        for stage in &plan.stages {
            if cancellation_token.is_cancelled() {
                break;
            }

            if let Some(blocked_by) = self.check_stage_dependencies(plan, stage, stage_success) {
                if !matches!(&failure, Some(StageFailure::BlockedByDependency { .. })) {
                    failure = Some(StageFailure::BlockedByDependency {
                        stage_id: stage.stage_id.clone(),
                        dependency: blocked_by,
                    });
                }
                continue;
            }

            if failure.is_some() {
                continue;
            }

            log::info!(
                "[OrchestratorEngine] Starting stage plan_id={} stage_id={} mode={} depends_on={:?} agent_count={}",
                plan.plan_id,
                stage.stage_id,
                stage.mode.as_str(),
                stage.depends_on,
                stage.agents.len()
            );

            self.emit_agent_spawns(plan, stage).await;

            let stage_results = self.run_stage(plan, stage, cancellation_token).await?;

            let all_success = stage_results.iter().all(|r| r.status == SubAgentStatus::Success);
            stage_success.insert(stage.stage_id.clone(), all_success);

            self.emit_stage_results(plan, stage, &stage_results, all_success).await;

            for result in stage_results {
                results.insert(result.agent_id.clone(), result);
            }

            if !all_success {
                failure = Some(StageFailure::StageFailed {
                    stage_id: stage.stage_id.clone(),
                });
            }
        }
        Ok(failure)
    }

    fn check_stage_dependencies(
        &self,
        plan: &OrchestrationPlan,
        stage: &planner::ExecutionStage,
        stage_success: &HashMap<String, bool>,
    ) -> Option<String> {
        for dep in &stage.depends_on {
            if !stage_success.get(dep).copied().unwrap_or(false) {
                log::warn!(
                    "[OrchestratorEngine] Stage blocked plan_id={} stage_id={} dependency={}",
                    plan.plan_id,
                    stage.stage_id,
                    dep
                );
                return Some(dep.clone());
            }
        }
        None
    }

    async fn emit_agent_spawns(&self, plan: &OrchestrationPlan, stage: &planner::ExecutionStage) {
        for agent in &stage.agents {
            log::info!(
                "[OrchestratorEngine] Spawning sub-agent plan_id={} stage_id={} agent_id={} type={} description={}",
                plan.plan_id,
                stage.stage_id,
                agent.agent_id,
                agent.subagent_type,
                agent.description
            );
            let spawn_args = SubAgentSpawnArgs {
                plan_id: plan.plan_id.clone(),
                agent_id: agent.agent_id.clone(),
                stage_id: stage.stage_id.clone(),
                description: agent.description.clone(),
                subagent_type: agent.subagent_type.clone(),
            };
            self.emit("sub_agent_spawn", &spawn_args).await;
        }
    }

    async fn run_stage(
        &self,
        plan: &OrchestrationPlan,
        stage: &planner::ExecutionStage,
        cancellation_token: &CancellationToken,
    ) -> Result<Vec<SubAgentResult>> {
        execute_stage(&plan.plan_id, stage, cancellation_token, {
            let executor = self.executor.clone();
            let ctx = self.tool_context.clone();
            let plan_id = plan.plan_id.clone();
            let event_tx = self.event_tx.clone();
            let catalog = self.catalog_agent_ids.clone();
            let default_id = self.default_agent_id.clone();
            move |agent_req, stage_id| {
                let executor = executor.clone();
                let ctx = ctx.clone();
                let plan_id = plan_id.clone();
                let event_tx = event_tx.clone();
                let catalog = catalog.clone();
                let default_id = default_id.clone();
                async move {
                    let ctx = rewire_log_forwarding(
                        ctx,
                        event_tx,
                        plan_id.clone(),
                        agent_req.agent_id.clone(),
                        stage_id.clone(),
                    );

                    let raw_selection = agent_req.agent_selection.as_deref().unwrap_or(&agent_req.subagent_type);
                    let validated_selection = if catalog.contains(raw_selection) {
                        raw_selection.to_string()
                    } else {
                        log::warn!(
                            "[OrchestratorEngine] Agent selection '{}' not in catalog, falling back to '{}'",
                            raw_selection,
                            default_id
                        );
                        default_id.clone()
                    };

                    let request = SubAgentRequest {
                        prompt: agent_req.prompt.clone(),
                        description: agent_req.description.clone(),
                        agent_selection: Some(validated_selection),
                        agent_id: Some(agent_req.agent_id.clone()),
                        plan_id: Some(plan_id.clone()),
                        stage_id: Some(stage_id.clone()),
                        output_format: Some(agent_req.output_format.clone().unwrap_or_else(|| "summary".to_string())),
                    };
                    let result = executor.execute_agent(request, ctx).await?;

                    Ok(SubAgentResult {
                        plan_id,
                        agent_id: agent_req.agent_id,
                        stage_id,
                        status: SubAgentStatus::Success,
                        output: result.output,
                        error: None,
                    })
                }
            }
        })
        .await
    }

    async fn emit_stage_results(
        &self,
        plan: &OrchestrationPlan,
        stage: &planner::ExecutionStage,
        stage_results: &[SubAgentResult],
        all_success: bool,
    ) {
        for result in stage_results {
            log::info!(
                "[OrchestratorEngine] Sub-agent finished plan_id={} stage_id={} agent_id={} status={} error={:?}",
                result.plan_id,
                result.stage_id,
                result.agent_id,
                result.status.as_str(),
                result.error
            );
            let complete_args = SubAgentCompleteArgs {
                plan_id: result.plan_id.clone(),
                agent_id: result.agent_id.clone(),
                stage_id: result.stage_id.clone(),
                status: result.status.as_str().to_string(),
                output_summary: result.output.clone(),
                error: result.error.clone(),
            };
            self.emit("sub_agent_complete", &complete_args).await;
        }

        let stage_complete = StageCompleteArgs {
            plan_id: plan.plan_id.clone(),
            stage_id: stage.stage_id.clone(),
            mode: stage.mode.as_str().to_string(),
            all_success,
        };
        self.emit("stage_complete", &stage_complete).await;

        log::info!(
            "[OrchestratorEngine] Stage complete plan_id={} stage_id={} all_success={}",
            plan.plan_id,
            stage.stage_id,
            all_success
        );
    }

    async fn run_review_with_retry(
        &self,
        plan: &OrchestrationPlan,
        results: &mut HashMap<String, SubAgentResult>,
        cancellation_token: &CancellationToken,
        max_retries: u32,
    ) -> Result<ReviewResult> {
        let review_start = OrchestrationReviewStartArgs {
            plan_id: plan.plan_id.clone(),
        };
        self.emit("orchestration_review_start", &review_start).await;

        log::info!(
            "[OrchestratorEngine] Starting review plan_id={} result_count={}",
            plan.plan_id,
            results.len()
        );

        let mut review = self.run_review(plan, results).await?;
        let mut retry_count = 0u32;

        while !review.success
            && !review.retry_agents.is_empty()
            && retry_count < max_retries
            && !cancellation_token.is_cancelled()
        {
            retry_count += 1;
            log::info!(
                "[OrchestratorEngine] Retrying agents plan_id={} retry={}/{} agents={:?}",
                plan.plan_id,
                retry_count,
                max_retries,
                review.retry_agents
            );

            self.retry_agents(plan, results, &review.retry_agents, cancellation_token)
                .await?;

            review = self.run_review(plan, results).await?;
        }

        log::info!(
            "[OrchestratorEngine] Review complete plan_id={} success={} retries={} summary={}",
            plan.plan_id,
            review.success,
            retry_count,
            review.summary
        );

        Ok(review)
    }

    async fn retry_agents(
        &self,
        plan: &OrchestrationPlan,
        results: &mut HashMap<String, SubAgentResult>,
        retry_agent_ids: &[String],
        cancellation_token: &CancellationToken,
    ) -> Result<()> {
        let agents_to_retry: Vec<_> = plan
            .stages
            .iter()
            .flat_map(|stage| {
                stage
                    .agents
                    .iter()
                    .filter(|agent| retry_agent_ids.contains(&agent.agent_id))
                    .map(|agent| (stage.stage_id.clone(), agent.clone()))
            })
            .collect();

        for (stage_id, agent_req) in agents_to_retry {
            if cancellation_token.is_cancelled() {
                break;
            }

            log::info!(
                "[OrchestratorEngine] Retrying agent plan_id={} stage_id={} agent_id={}",
                plan.plan_id,
                stage_id,
                agent_req.agent_id
            );

            let spawn_args = SubAgentSpawnArgs {
                plan_id: plan.plan_id.clone(),
                agent_id: agent_req.agent_id.clone(),
                stage_id: stage_id.clone(),
                description: format!("[retry] {}", agent_req.description),
                subagent_type: agent_req.subagent_type.clone(),
            };
            self.emit("sub_agent_spawn", &spawn_args).await;

            let ctx = rewire_log_forwarding(
                self.tool_context.clone(),
                self.event_tx.clone(),
                plan.plan_id.clone(),
                agent_req.agent_id.clone(),
                stage_id.clone(),
            );

            let raw_selection = agent_req.agent_selection.as_deref().unwrap_or(&agent_req.subagent_type);
            let validated_selection = if self.catalog_agent_ids.contains(raw_selection) {
                raw_selection.to_string()
            } else {
                log::warn!(
                    "[OrchestratorEngine] Retry agent selection '{}' not in catalog, falling back to '{}'",
                    raw_selection,
                    self.default_agent_id
                );
                self.default_agent_id.clone()
            };

            let request = SubAgentRequest {
                prompt: agent_req.prompt.clone(),
                description: agent_req.description.clone(),
                agent_selection: Some(validated_selection),
                agent_id: Some(agent_req.agent_id.clone()),
                plan_id: Some(plan.plan_id.clone()),
                stage_id: Some(stage_id.clone()),
                output_format: Some(agent_req.output_format.clone().unwrap_or_else(|| "summary".to_string())),
            };

            let result = match self.executor.execute_agent(request, ctx).await {
                Ok(output) => SubAgentResult {
                    plan_id: plan.plan_id.clone(),
                    agent_id: agent_req.agent_id.clone(),
                    stage_id: stage_id.clone(),
                    status: SubAgentStatus::Success,
                    output: output.output,
                    error: None,
                },
                Err(e) => SubAgentResult {
                    plan_id: plan.plan_id.clone(),
                    agent_id: agent_req.agent_id.clone(),
                    stage_id: stage_id.clone(),
                    status: SubAgentStatus::Failed,
                    output: String::new(),
                    error: Some(e.to_string()),
                },
            };

            let complete_args = SubAgentCompleteArgs {
                plan_id: result.plan_id.clone(),
                agent_id: result.agent_id.clone(),
                stage_id: result.stage_id.clone(),
                status: result.status.as_str().to_string(),
                output_summary: result.output.clone(),
                error: result.error.clone(),
            };
            self.emit("sub_agent_complete", &complete_args).await;

            results.insert(result.agent_id.clone(), result);
        }
        Ok(())
    }

    async fn run_review(
        &self,
        plan: &OrchestrationPlan,
        results: &HashMap<String, SubAgentResult>,
    ) -> Result<ReviewResult> {
        let prompt = build_review_prompt(&plan.description, results);
        log::info!(
            "[OrchestratorEngine] Built review prompt plan_id={} prompt_chars={} result_count={}",
            plan.plan_id,
            prompt.chars().count(),
            results.len()
        );

        let review_agent_id = self.review_agent_id();
        let request = SubAgentRequest {
            prompt,
            description: "Review orchestration outputs".to_string(),
            agent_selection: Some(review_agent_id),
            agent_id: None,
            plan_id: None,
            stage_id: None,
            output_format: None,
        };
        match self.executor.execute_agent(request, self.tool_context.clone()).await {
            Ok(result) => {
                log::debug!(
                    "[OrchestratorEngine] Review agent raw output plan_id={} chars={} output={:?}",
                    plan.plan_id,
                    result.output.chars().count(),
                    result.output.chars().take(500).collect::<String>()
                );
                match parse_review_result(&result.output) {
                    Ok(review) => Ok(review),
                    Err(e) => {
                        log::warn!(
                            "[OrchestratorEngine] Review parse failed plan_id={} error={} raw_output={:?}",
                            plan.plan_id,
                            e,
                            result.output.chars().take(300).collect::<String>()
                        );
                        Ok(ReviewResult {
                            success: true,
                            issues: vec![format!("Review parse error: {e}")],
                            summary: format!("Review result unparseable (treating as pass): {e}"),
                            retry_agents: vec![],
                        })
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[OrchestratorEngine] Review agent failed plan_id={} error={}; treating as pass-through",
                    plan.plan_id,
                    e
                );
                Ok(ReviewResult {
                    success: true,
                    issues: vec![format!("Review executor error: {e}")],
                    summary: format!("Review skipped due to executor error: {e}"),
                    retry_agents: vec![],
                })
            }
        }
    }

    async fn emit_plan_event(&self, plan: &OrchestrationPlan) {
        let plan_event = OrchestrationPlanEvent {
            plan_id: plan.plan_id.clone(),
            description: plan.description.clone(),
            stages: plan
                .stages
                .iter()
                .map(|stage| StageSummary {
                    stage_id: stage.stage_id.clone(),
                    mode: stage.mode.as_str().to_string(),
                    depends_on: stage.depends_on.clone(),
                    agents: stage
                        .agents
                        .iter()
                        .map(|agent| AgentSummary {
                            agent_id: agent.agent_id.clone(),
                            description: agent.description.clone(),
                            subagent_type: agent.subagent_type.clone(),
                            agent_selection: agent.agent_selection.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        self.emit("orchestration_plan", &plan_event).await;
    }

    async fn emit_complete(&self, plan_id: &str, success: bool, summary: &str) {
        let complete = OrchestrationCompleteArgs {
            plan_id: plan_id.to_string(),
            overall_success: success,
            summary: summary.to_string(),
        };
        self.emit("orchestration_complete", &complete).await;
    }

    fn emit_sync(&self, kind: &str, args: &impl serde::Serialize) {
        let payload = match serde_json::to_value(args) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "[OrchestratorEngine] Failed to serialize event args for kind={}: {}",
                    kind,
                    e
                );
                return;
            }
        };
        if let Err(e) = self.event_tx.try_send(AgentEvent::OrchestrationProgress {
            kind: kind.to_string(),
            payload,
        }) {
            log::warn!("[OrchestratorEngine] Failed to emit event kind={}: {}", kind, e);
        }
    }

    async fn emit(&self, kind: &str, args: &impl serde::Serialize) {
        self.emit_sync(kind, args);
    }
}

/// Creates a ToolContext that intercepts LogDelta events from the sub-agent and re-emits
/// them as `sub_agent_log` OrchestrationProgress events for the frontend orchestration UI.
fn rewire_log_forwarding(
    ctx: Option<ToolContext>,
    orchestrator_tx: mpsc::Sender<AgentEvent>,
    plan_id: String,
    agent_id: String,
    stage_id: String,
) -> Option<ToolContext> {
    let ctx = ctx?;
    let (interceptor_tx, mut interceptor_rx) = mpsc::channel::<AgentEvent>(256);
    let original_tx = ctx.event_tx.clone();

    tokio::spawn(async move {
        while let Some(event) = interceptor_rx.recv().await {
            if let AgentEvent::LogDelta {
                ref log, ref stream, ..
            } = event
            {
                let preview = if log.chars().count() > 120 {
                    format!("{}...", log.chars().take(120).collect::<String>())
                } else {
                    log.clone()
                };
                log::trace!(
                    "[OrchestratorEngine] Forwarding sub-agent log plan_id={} stage_id={} agent_id={} stream={:?} preview={}",
                    plan_id,
                    stage_id,
                    agent_id,
                    stream,
                    preview
                );

                let payload = serde_json::json!({
                    "planId": plan_id,
                    "agentId": agent_id,
                    "stageId": stage_id,
                    "stream": stream,
                    "log": log,
                });
                let _ = orchestrator_tx.try_send(AgentEvent::OrchestrationProgress {
                    kind: "sub_agent_log".to_string(),
                    payload,
                });
            }
            let _ = original_tx.send(event).await;
        }
    });

    Some(ToolContext {
        event_tx: interceptor_tx,
        ..ctx
    })
}

#[cfg(test)]
mod tests {
    use super::{rewire_log_forwarding, OrchestratorEngine, SubAgentExecutor, SubAgentOutput, SubAgentRequest};
    use crate::event::AgentEvent;
    use crate::orchestrator::planner;
    use crate::orchestrator::scheduler::SubAgentStatus;
    use crate::prompt::EnvironmentSnapshot;
    use crate::tool::ToolContext;
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};
    use tokio::time::{timeout, Duration};
    use tokio_util::sync::CancellationToken;

    #[derive(Clone)]
    struct AgentSpec {
        agent_id: String,
        description: String,
        subagent_type: String,
        agent_selection: Option<String>,
    }

    #[derive(Clone)]
    struct StageSpec {
        stage_id: String,
        mode: &'static str,
        depends_on: Vec<String>,
        agents: Vec<AgentSpec>,
    }

    #[derive(Debug, Clone)]
    struct ParsedEvent {
        kind: String,
        args: Value,
    }

    #[derive(Clone)]
    struct MockExecutor {
        responses: HashMap<String, std::result::Result<String, String>>,
        catalog: HashSet<String>,
        default_id: String,
        cancel_on_key: Option<(String, CancellationToken)>,
    }

    #[async_trait]
    impl SubAgentExecutor for MockExecutor {
        async fn execute_agent(
            &self,
            request: SubAgentRequest,
            _context: Option<ToolContext>,
        ) -> Result<SubAgentOutput> {
            let key = request_key(&request);
            if let Some((cancel_key, token)) = &self.cancel_on_key {
                if key == *cancel_key {
                    token.cancel();
                }
            }

            match self.responses.get(&key) {
                Some(Ok(output)) => Ok(SubAgentOutput {
                    output: output.clone(),
                    duration_ms: 0,
                    warnings: vec![],
                }),
                Some(Err(message)) => Err(anyhow!(message.clone())),
                None => Ok(SubAgentOutput {
                    output: String::new(),
                    duration_ms: 0,
                    warnings: vec![],
                }),
            }
        }

        fn catalog_agent_ids(&self) -> HashSet<String> {
            self.catalog.clone()
        }

        fn default_agent_id(&self) -> String {
            self.default_id.clone()
        }
    }

    fn request_key(request: &SubAgentRequest) -> String {
        let reviewer_selected = request
            .agent_selection
            .as_deref()
            .map(|value| value.eq_ignore_ascii_case("reviewer"))
            .unwrap_or(false);
        if reviewer_selected {
            return "reviewer".to_string();
        }

        request
            .agent_id
            .as_deref()
            .or(Some(request.description.as_str()))
            .unwrap_or_default()
            .to_string()
    }

    fn build_executor(
        responses: HashMap<String, std::result::Result<String, String>>,
        catalog: &[&str],
        default_id: &str,
    ) -> Arc<dyn SubAgentExecutor> {
        Arc::new(MockExecutor {
            responses,
            catalog: catalog.iter().map(|item| (*item).to_string()).collect(),
            default_id: default_id.to_string(),
            cancel_on_key: None,
        })
    }

    fn build_executor_with_cancellation(
        responses: HashMap<String, std::result::Result<String, String>>,
        catalog: &[&str],
        default_id: &str,
        cancel_key: &str,
        token: CancellationToken,
    ) -> Arc<dyn SubAgentExecutor> {
        Arc::new(MockExecutor {
            responses,
            catalog: catalog.iter().map(|item| (*item).to_string()).collect(),
            default_id: default_id.to_string(),
            cancel_on_key: Some((cancel_key.to_string(), token)),
        })
    }

    fn build_engine(
        executor: Arc<dyn SubAgentExecutor>,
    ) -> (OrchestratorEngine, mpsc::Receiver<AgentEvent>, Option<ToolContext>) {
        let (event_tx, event_rx) = mpsc::channel(64);
        let context = Some(build_tool_context(event_tx.clone(), None));
        let engine = OrchestratorEngine::new(executor, event_tx, context.clone());
        (engine, event_rx, context)
    }

    fn build_tool_context(
        event_tx: mpsc::Sender<AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> ToolContext {
        ToolContext {
            event_tx,
            tool_use_id: "tool-1".to_string(),
            session_id: "session-1".to_string(),
            task_store: None,
            skill_registry: None,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            turn_read_state: None,
            environment: Some(EnvironmentSnapshot {
                config_dir: "D:/config".to_string(),
                project_dir: None,
                platform: "windows".to_string(),
                shell: "powershell".to_string(),
                git_branch: None,
                git_status_summary: None,
                recent_commits: None,
                model_id: None,
                current_date: "2026-05-16".to_string(),
            }),
            shared_environment: None,
            cancellation_token,
            visible_tool_names: Arc::new(HashSet::new()),
        }
    }

    fn build_plan_json(stages: Vec<StageSpec>) -> String {
        serde_json::to_string(&json!({
            "planId": "p1",
            "description": "test plan",
            "stages": stages.into_iter().map(stage_to_json).collect::<Vec<_>>()
        }))
        .expect("serialize plan")
    }

    fn stage_to_json(stage: StageSpec) -> Value {
        json!({
            "stageId": stage.stage_id,
            "mode": stage.mode,
            "dependsOn": stage.depends_on,
            "agents": stage.agents.into_iter().map(agent_to_json).collect::<Vec<_>>()
        })
    }

    fn agent_to_json(agent: AgentSpec) -> Value {
        json!({
            "agentId": agent.agent_id,
            "description": agent.description,
            "subagentType": agent.subagent_type,
            "agentSelection": agent.agent_selection,
            "prompt": "do work",
            "contextFiles": [],
            "outputFormat": "summary"
        })
    }

    fn review_response(success: bool, summary: &str) -> String {
        serde_json::to_string(&json!({
            "success": success,
            "issues": [],
            "retryAgents": [],
            "summary": summary
        }))
        .expect("serialize review response")
    }

    fn collect_events(rx: &mut mpsc::Receiver<AgentEvent>) -> Vec<ParsedEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AgentEvent::OrchestrationProgress { kind, payload } = event {
                events.push(ParsedEvent { kind, args: payload });
            }
        }
        events
    }

    fn event_kinds(events: &[ParsedEvent]) -> Vec<&str> {
        events.iter().map(|event| event.kind.as_str()).collect()
    }

    fn find_event<'a>(events: &'a [ParsedEvent], kind: &str) -> &'a ParsedEvent {
        events
            .iter()
            .find(|event| event.kind == kind)
            .expect("event should exist")
    }

    fn stage(stage_id: &str, mode: &'static str, depends_on: Vec<&str>, agents: Vec<AgentSpec>) -> StageSpec {
        StageSpec {
            stage_id: stage_id.to_string(),
            mode,
            depends_on: depends_on.into_iter().map(|item| item.to_string()).collect(),
            agents,
        }
    }

    fn agent(agent_id: &str) -> AgentSpec {
        AgentSpec {
            agent_id: agent_id.to_string(),
            description: format!("task-{agent_id}"),
            subagent_type: "nova".to_string(),
            agent_selection: Some("nova".to_string()),
        }
    }

    fn parse_plan(stages: Vec<StageSpec>) -> crate::orchestrator::planner::OrchestrationPlan {
        let plan_json = build_plan_json(stages);
        planner::parse_and_validate(&plan_json).expect("plan should parse")
    }

    #[test]
    fn validate_agent_selection_known_agent() {
        let executor = build_executor(HashMap::new(), &["nova", "developer"], "nova");
        let (engine, _event_rx, _context) = build_engine(executor);

        let (selected, warning) = engine.validate_agent_selection("developer");
        assert_eq!(selected, "developer");
        assert!(warning.is_none());
    }

    #[test]
    fn validate_agent_selection_unknown_agent() {
        let executor = build_executor(HashMap::new(), &["nova"], "nova");
        let (engine, _event_rx, _context) = build_engine(executor);

        let (selected, warning) = engine.validate_agent_selection("unknown-agent");
        assert_eq!(selected, "nova");
        let warning = warning.expect("warning should exist");
        assert!(warning.contains("not found in catalog"));
    }

    #[test]
    fn review_agent_id_with_reviewer() {
        let executor = build_executor(HashMap::new(), &["nova", "reviewer"], "nova");
        let (engine, _event_rx, _context) = build_engine(executor);

        assert_eq!(engine.review_agent_id(), "reviewer");
    }

    #[test]
    fn review_agent_id_without_reviewer() {
        let executor = build_executor(HashMap::new(), &["nova"], "nova");
        let (engine, _event_rx, _context) = build_engine(executor);

        assert_eq!(engine.review_agent_id(), "nova");
    }

    #[tokio::test]
    async fn single_stage_parallel_all_success() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("a2".to_string(), Ok("output-a2".to_string()));
        responses.insert(
            "reviewer".to_string(),
            Ok(review_response(true, "All agents completed successfully.")),
        );
        let executor = build_executor(responses, &["nova", "reviewer"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![stage("s1", "parallel", vec![], vec![agent("a1"), agent("a2")])]);

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results["a1"].status, SubAgentStatus::Success);
        assert_eq!(outcome.results["a2"].status, SubAgentStatus::Success);
        let review = outcome.review.expect("review should exist");
        assert!(review.success);

        let events = collect_events(&mut event_rx);
        let kinds = event_kinds(&events);
        assert_eq!(kinds.first().copied(), Some("orchestration_plan"));
        assert_eq!(kinds.iter().filter(|kind| **kind == "sub_agent_spawn").count(), 2);
        assert_eq!(kinds.iter().filter(|kind| **kind == "sub_agent_complete").count(), 2);
        assert!(kinds.contains(&"stage_complete"));
        assert!(kinds.contains(&"orchestration_review_start"));
        assert_eq!(kinds.last().copied(), Some("orchestration_complete"));
        assert_eq!(
            find_event(&events, "orchestration_complete").args["overallSuccess"],
            Value::Bool(true)
        );
    }

    #[tokio::test]
    async fn single_stage_serial_all_success() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("a2".to_string(), Ok("output-a2".to_string()));
        responses.insert(
            "reviewer".to_string(),
            Ok(review_response(true, "All agents completed successfully.")),
        );
        let executor = build_executor(responses, &["nova", "reviewer"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![stage("s1", "serial", vec![], vec![agent("a1"), agent("a2")])]);

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results["a1"].status, SubAgentStatus::Success);
        assert_eq!(outcome.results["a2"].status, SubAgentStatus::Success);
        assert!(outcome.review.expect("review should exist").success);

        let events = collect_events(&mut event_rx);
        let kinds = event_kinds(&events);
        assert_eq!(kinds.first().copied(), Some("orchestration_plan"));
        assert_eq!(kinds.iter().filter(|kind| **kind == "sub_agent_spawn").count(), 2);
        assert_eq!(kinds.iter().filter(|kind| **kind == "sub_agent_complete").count(), 2);
        assert_eq!(kinds.last().copied(), Some("orchestration_complete"));
    }

    #[tokio::test]
    async fn two_stage_serial_dependency() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("a2".to_string(), Ok("output-a2".to_string()));
        responses.insert("reviewer".to_string(), Ok(review_response(true, "Looks good.")));
        let executor = build_executor(responses, &["nova", "reviewer"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![
            stage("s1", "parallel", vec![], vec![agent("a1")]),
            stage("s2", "serial", vec!["s1"], vec![agent("a2")]),
        ]);

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        assert!(outcome.review.expect("review should exist").success);
        let events = collect_events(&mut event_rx);
        let s1_spawn = events
            .iter()
            .position(|event| event.kind == "sub_agent_spawn" && event.args["stageId"] == "s1")
            .expect("s1 spawn should exist");
        let s2_spawn = events
            .iter()
            .position(|event| event.kind == "sub_agent_spawn" && event.args["stageId"] == "s2")
            .expect("s2 spawn should exist");
        assert!(s1_spawn < s2_spawn);
        assert_eq!(events.iter().filter(|event| event.kind == "stage_complete").count(), 2);
        assert_eq!(
            find_event(&events, "orchestration_complete").args["overallSuccess"],
            Value::Bool(true)
        );
    }

    #[tokio::test]
    async fn dependency_failure_blocks_downstream() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Err("boom".to_string()));
        let executor = build_executor(responses, &["nova"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![
            stage("s1", "parallel", vec![], vec![agent("a1")]),
            stage("s2", "serial", vec!["s1"], vec![agent("a2")]),
        ]);

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should finish");

        assert!(outcome.review.is_none());
        let events = collect_events(&mut event_rx);
        let stage_complete = find_event(&events, "stage_complete");
        assert_eq!(stage_complete.args["allSuccess"], Value::Bool(false));
        assert!(!events
            .iter()
            .any(|event| event.kind == "sub_agent_spawn" && event.args["stageId"] == "s2"));
        let complete = find_event(&events, "orchestration_complete");
        assert_eq!(complete.args["overallSuccess"], Value::Bool(false));
        let summary = complete.args["summary"].as_str().expect("summary should be string");
        assert!(summary.contains("blocked by dependency"));
    }

    #[tokio::test]
    async fn partial_stage_failure_stops_orchestration() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("a2".to_string(), Err("fail".to_string()));
        let executor = build_executor(responses, &["nova"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![
            stage("s1", "parallel", vec![], vec![agent("a1"), agent("a2")]),
            stage("s2", "serial", vec!["s1"], vec![agent("a3")]),
        ]);

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should finish");

        assert!(outcome.review.is_none());
        let events = collect_events(&mut event_rx);
        assert_eq!(
            find_event(&events, "stage_complete").args["allSuccess"],
            Value::Bool(false)
        );
        assert!(!events
            .iter()
            .any(|event| event.kind == "sub_agent_spawn" && event.args["stageId"] == "s2"));
        assert_eq!(
            find_event(&events, "orchestration_complete").args["overallSuccess"],
            Value::Bool(false)
        );
    }

    #[tokio::test]
    async fn cancellation_before_review() {
        let token = CancellationToken::new();
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        let executor = build_executor_with_cancellation(responses, &["nova"], "nova", "a1", token.clone());
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![stage("s1", "parallel", vec![], vec![agent("a1")])]);

        let outcome = engine
            .execute_plan(plan, token)
            .await
            .expect("execute plan should finish");

        assert!(outcome.review.is_none());
        let events = collect_events(&mut event_rx);
        let complete = find_event(&events, "orchestration_complete");
        assert_eq!(complete.args["overallSuccess"], Value::Bool(false));
        assert!(complete.args["summary"]
            .as_str()
            .expect("summary should be string")
            .contains("cancelled"));
    }

    #[tokio::test]
    async fn empty_plan_no_stages() {
        let executor = build_executor(HashMap::new(), &["nova"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = planner::parse_and_validate(
            &serde_json::to_string(&json!({
                "planId": "test",
                "description": "empty",
                "stages": []
            }))
            .expect("serialize empty plan"),
        )
        .expect("plan should parse");

        let outcome = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        assert!(outcome.results.is_empty());
        assert!(outcome.review.is_none());
        let events = collect_events(&mut event_rx);
        let complete = find_event(&events, "orchestration_complete");
        assert_eq!(complete.args["overallSuccess"], Value::Bool(true));
        assert!(complete.args["summary"]
            .as_str()
            .expect("summary should be string")
            .contains("No stages"));
    }

    #[tokio::test]
    async fn event_sequence_single_stage() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("reviewer".to_string(), Ok(review_response(true, "All good.")));
        let executor = build_executor(responses, &["nova", "reviewer"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = parse_plan(vec![stage("s1", "parallel", vec![], vec![agent("a1")])]);

        let _ = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        let events = collect_events(&mut event_rx);
        assert_eq!(
            event_kinds(&events),
            vec![
                "orchestration_plan",
                "sub_agent_spawn",
                "sub_agent_complete",
                "stage_complete",
                "orchestration_review_start",
                "orchestration_complete",
            ]
        );
    }

    #[tokio::test]
    async fn event_args_contain_correct_ids() {
        let mut responses = HashMap::new();
        responses.insert("a1".to_string(), Ok("output-a1".to_string()));
        responses.insert("reviewer".to_string(), Ok(review_response(true, "All good.")));
        let executor = build_executor(responses, &["nova", "reviewer"], "nova");
        let (engine, mut event_rx, _context) = build_engine(executor);
        let plan = planner::parse_and_validate(
            &serde_json::to_string(&json!({
                "planId": "p1",
                "description": "ids",
                "stages": [{
                    "stageId": "s1",
                    "mode": "parallel",
                    "dependsOn": [],
                    "agents": [{
                        "agentId": "a1",
                        "description": "task-a1",
                        "subagentType": "nova",
                        "agentSelection": "nova",
                        "prompt": "do work"
                    }]
                }]
            }))
            .expect("serialize plan"),
        )
        .expect("plan should parse");

        let _ = engine
            .execute_plan(plan, CancellationToken::new())
            .await
            .expect("execute plan should succeed");

        let events = collect_events(&mut event_rx);
        assert_eq!(
            find_event(&events, "orchestration_plan").args["planId"],
            Value::String("p1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_spawn").args["planId"],
            Value::String("p1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_spawn").args["stageId"],
            Value::String("s1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_spawn").args["agentId"],
            Value::String("a1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_complete").args["planId"],
            Value::String("p1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_complete").args["stageId"],
            Value::String("s1".to_string())
        );
        assert_eq!(
            find_event(&events, "sub_agent_complete").args["agentId"],
            Value::String("a1".to_string())
        );
        assert_eq!(
            find_event(&events, "stage_complete").args["planId"],
            Value::String("p1".to_string())
        );
        assert_eq!(
            find_event(&events, "stage_complete").args["stageId"],
            Value::String("s1".to_string())
        );
        assert_eq!(
            find_event(&events, "orchestration_complete").args["planId"],
            Value::String("p1".to_string())
        );
    }

    #[tokio::test]
    async fn log_delta_forwarded_as_orchestration_progress() {
        let (orchestrator_tx, mut orchestrator_rx) = mpsc::channel(8);
        let (original_tx, mut original_rx) = mpsc::channel(8);
        let context = Some(build_tool_context(original_tx, None));
        let forwarded = rewire_log_forwarding(
            context,
            orchestrator_tx,
            "p1".to_string(),
            "a1".to_string(),
            "s1".to_string(),
        )
        .expect("context should be rewired");

        forwarded
            .event_tx
            .send(AgentEvent::LogDelta {
                id: "tool-1".to_string(),
                name: "Agent".to_string(),
                log: "hello".to_string(),
                stream: "stdout".to_string(),
            })
            .await
            .expect("send log delta");

        let orchestrator_event = timeout(Duration::from_millis(200), orchestrator_rx.recv())
            .await
            .expect("orchestrator event should arrive")
            .expect("orchestrator channel should yield event");
        let original_event = timeout(Duration::from_millis(200), original_rx.recv())
            .await
            .expect("original event should arrive")
            .expect("original channel should yield event");

        match orchestrator_event {
            AgentEvent::OrchestrationProgress { kind, payload } => {
                assert_eq!(kind, "sub_agent_log");
                assert_eq!(payload["planId"], Value::String("p1".to_string()));
                assert_eq!(payload["stageId"], Value::String("s1".to_string()));
                assert_eq!(payload["agentId"], Value::String("a1".to_string()));
                assert_eq!(payload["log"], Value::String("hello".to_string()));
            }
            other => panic!("unexpected orchestrator event: {other:?}"),
        }

        match original_event {
            AgentEvent::LogDelta { log, stream, .. } => {
                assert_eq!(log, "hello");
                assert_eq!(stream, "stdout");
            }
            other => panic!("unexpected original event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_log_events_passthrough_only() {
        let (orchestrator_tx, mut orchestrator_rx) = mpsc::channel(8);
        let (original_tx, mut original_rx) = mpsc::channel(8);
        let context = Some(build_tool_context(original_tx, None));
        let forwarded = rewire_log_forwarding(
            context,
            orchestrator_tx,
            "p1".to_string(),
            "a1".to_string(),
            "s1".to_string(),
        )
        .expect("context should be rewired");

        forwarded
            .event_tx
            .send(AgentEvent::TextDelta("chunk".to_string()))
            .await
            .expect("send text delta");

        let original_event = timeout(Duration::from_millis(200), original_rx.recv())
            .await
            .expect("original event should arrive")
            .expect("original channel should yield event");
        match original_event {
            AgentEvent::TextDelta(chunk) => assert_eq!(chunk, "chunk"),
            other => panic!("unexpected original event: {other:?}"),
        }

        let orchestrator_result = timeout(Duration::from_millis(50), orchestrator_rx.recv()).await;
        assert!(
            orchestrator_result.is_err(),
            "orchestrator should not receive non-log events"
        );
    }

    #[test]
    fn none_context_returns_none() {
        let (orchestrator_tx, _orchestrator_rx) = mpsc::channel(1);
        let result = rewire_log_forwarding(
            None,
            orchestrator_tx,
            "p1".to_string(),
            "a1".to_string(),
            "s1".to_string(),
        );
        assert!(result.is_none());
    }
}
