pub mod planner;
pub mod reviewer;
pub mod scheduler;

use crate::event::AgentEvent;
use crate::tool::builtin::agent::AgentTool;
use crate::tool::{Tool, ToolContext};
use anyhow::{anyhow, Result};
use nova_protocol::orchestration::{
    AgentSummary, OrchestrationCompleteArgs, OrchestrationPlanEvent, OrchestrationReviewStartArgs, StageCompleteArgs,
    StageSummary, SubAgentCompleteArgs, SubAgentSpawnArgs,
};
use planner::{parse_and_validate, OrchestrationPlan};
use reviewer::{build_review_prompt, parse_review_result, ReviewResult};
use scheduler::{execute_stage, SubAgentResult, SubAgentStatus};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct OrchestratorEngine {
    agent_tool: Arc<AgentTool>,
    event_tx: mpsc::Sender<AgentEvent>,
    tool_context: Option<ToolContext>,
}

pub struct ExecutionOutcome {
    pub plan: OrchestrationPlan,
    pub results: HashMap<String, SubAgentResult>,
    pub review: Option<ReviewResult>,
}

impl OrchestratorEngine {
    pub fn new(
        agent_tool: Arc<AgentTool>,
        event_tx: mpsc::Sender<AgentEvent>,
        tool_context: Option<ToolContext>,
    ) -> Self {
        Self {
            agent_tool,
            event_tx,
            tool_context,
        }
    }

    pub fn parse_plan(&self, plan_json: &str) -> Result<OrchestrationPlan> {
        parse_and_validate(plan_json)
    }

    pub async fn execute_plan(
        &self,
        plan_json: &str,
        cancellation_token: CancellationToken,
    ) -> Result<ExecutionOutcome> {
        let plan = self.parse_plan(plan_json)?;
        let mut results = HashMap::<String, SubAgentResult>::new();
        let mut stage_success = HashMap::<String, bool>::new();

        log::info!(
            "[OrchestratorEngine] Parsed plan_id={} stage_count={} description={}",
            plan.plan_id,
            plan.stages.len(),
            plan.description
        );

        // Emit orchestration_plan event using typed struct
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
                        })
                        .collect(),
                })
                .collect(),
        };
        self.emit("orchestration_plan", &plan_event).await;

        for stage in &plan.stages {
            log::info!(
                "[OrchestratorEngine] Starting stage plan_id={} stage_id={} mode={} depends_on={:?} agent_count={}",
                plan.plan_id,
                stage.stage_id,
                stage.mode.as_str(),
                stage.depends_on,
                stage.agents.len()
            );

            for dep in &stage.depends_on {
                if !stage_success.get(dep).copied().unwrap_or(false) {
                    log::warn!(
                        "[OrchestratorEngine] Stage blocked plan_id={} stage_id={} dependency={} dependency_success={:?}",
                        plan.plan_id,
                        stage.stage_id,
                        dep,
                        stage_success.get(dep)
                    );

                    let complete = OrchestrationCompleteArgs {
                        plan_id: plan.plan_id.clone(),
                        overall_success: false,
                        summary: format!("Stage '{}' blocked by dependency '{}'.", stage.stage_id, dep),
                    };
                    self.emit("orchestration_complete", &complete).await;

                    return Ok(ExecutionOutcome {
                        plan,
                        results,
                        review: None,
                    });
                }
            }

            // Emit sub_agent_spawn events before executing the stage
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

            let stage_results = execute_stage(&plan.plan_id, stage, &cancellation_token, {
                let tool = self.agent_tool.clone();
                let ctx = self.tool_context.clone();
                let plan_id = plan.plan_id.clone();
                let event_tx = self.event_tx.clone();
                move |agent_req, stage_id| {
                    let tool = tool.clone();
                    let ctx = ctx.clone();
                    let plan_id = plan_id.clone();
                    let event_tx = event_tx.clone();
                    async move {
                        // Build a ToolContext that intercepts LogDelta and re-emits as sub_agent_log
                        let ctx = rewire_log_forwarding(
                            ctx,
                            event_tx,
                            plan_id.clone(),
                            agent_req.agent_id.clone(),
                            stage_id.clone(),
                        );

                        let output = tool
                            .execute(
                                json!({
                                    "prompt": agent_req.prompt,
                                    "description": agent_req.description,
                                    "subagent_type": agent_req.subagent_type,
                                    "run_in_background": false,
                                    "agent_id": agent_req.agent_id,
                                    "parent_plan_id": plan_id,
                                    "stage_id": stage_id,
                                    "output_format": agent_req.output_format.unwrap_or_else(|| "summary".to_string())
                                }),
                                ctx,
                            )
                            .await?;

                        let parsed: serde_json::Value = serde_json::from_str(&output.content)
                            .map_err(|e| anyhow!("failed to parse sub-agent output: {}", e))?;
                        let content = parsed
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();

                        Ok(SubAgentResult {
                            plan_id,
                            agent_id: agent_req.agent_id,
                            stage_id,
                            status: SubAgentStatus::Success,
                            output: content,
                            error: None,
                        })
                    }
                }
            })
            .await?;

            let all_success = stage_results.iter().all(|r| r.status == SubAgentStatus::Success);
            stage_success.insert(stage.stage_id.clone(), all_success);

            for result in &stage_results {
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

            for result in stage_results {
                results.insert(result.agent_id.clone(), result);
            }

            if cancellation_token.is_cancelled() || !all_success {
                break;
            }
        }

        if results.is_empty() {
            log::info!("[OrchestratorEngine] No stages scheduled for plan_id={}", plan.plan_id);

            let complete = OrchestrationCompleteArgs {
                plan_id: plan.plan_id.clone(),
                overall_success: true,
                summary: "No stages were scheduled.".to_string(),
            };
            self.emit("orchestration_complete", &complete).await;

            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        if cancellation_token.is_cancelled() {
            log::warn!(
                "[OrchestratorEngine] Orchestration cancelled before review plan_id={} result_count={}",
                plan.plan_id,
                results.len()
            );

            let complete = OrchestrationCompleteArgs {
                plan_id: plan.plan_id.clone(),
                overall_success: false,
                summary: "Orchestration cancelled before review.".to_string(),
            };
            self.emit("orchestration_complete", &complete).await;

            return Ok(ExecutionOutcome {
                plan,
                results,
                review: None,
            });
        }

        let review_start = OrchestrationReviewStartArgs {
            plan_id: plan.plan_id.clone(),
        };
        self.emit("orchestration_review_start", &review_start).await;

        log::info!(
            "[OrchestratorEngine] Starting review plan_id={} result_count={}",
            plan.plan_id,
            results.len()
        );

        let review = self.run_review(&plan, &results).await?;

        log::info!(
            "[OrchestratorEngine] Review complete plan_id={} success={} summary={}",
            plan.plan_id,
            review.success,
            review.summary
        );

        let complete = OrchestrationCompleteArgs {
            plan_id: plan.plan_id.clone(),
            overall_success: review.success,
            summary: review.summary.clone(),
        };
        self.emit("orchestration_complete", &complete).await;

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

        let output = self
            .agent_tool
            .execute(
                json!({
                    "prompt": prompt,
                    "description": "Review orchestration outputs",
                    "subagent_type": "Reviewer",
                    "run_in_background": false,
                }),
                self.tool_context.clone(),
            )
            .await?;

        let parsed: serde_json::Value = serde_json::from_str(&output.content)?;
        let raw_review = parsed
            .get("output")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("missing review output content"))?;
        parse_review_result(raw_review)
    }

    async fn emit(&self, kind: &str, args: &impl serde::Serialize) {
        let args_value = match serde_json::to_value(args) {
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
        let _ = self
            .event_tx
            .send(AgentEvent::OrchestrationProgress {
                kind: kind.to_string(),
                args: args_value,
                log: None,
                stream: None,
            })
            .await;
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
            // Forward log events as orchestration sub_agent_log
            if let AgentEvent::LogDelta {
                ref log, ref stream, ..
            } = event
            {
                let preview = if log.chars().count() > 120 {
                    format!("{}...", log.chars().take(120).collect::<String>())
                } else {
                    log.clone()
                };
                log::info!(
                    "[OrchestratorEngine] Forwarding sub-agent log plan_id={} stage_id={} agent_id={} stream={:?} preview={}",
                    plan_id,
                    stage_id,
                    agent_id,
                    stream,
                    preview
                );

                let _ = orchestrator_tx
                    .send(AgentEvent::OrchestrationProgress {
                        kind: "sub_agent_log".to_string(),
                        args: serde_json::to_value(&nova_protocol::orchestration::SubAgentLogArgs {
                            plan_id: plan_id.clone(),
                            agent_id: agent_id.clone(),
                            stage_id: stage_id.clone(),
                        })
                        .unwrap_or_default(),
                        log: Some(log.clone()),
                        stream: Some(stream.clone()),
                    })
                    .await;
            }
            // Always forward to original parent
            let _ = original_tx.send(event).await;
        }
    });

    Some(ToolContext {
        event_tx: interceptor_tx,
        ..ctx
    })
}
