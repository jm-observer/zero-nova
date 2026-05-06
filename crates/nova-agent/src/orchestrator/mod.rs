pub mod planner;
pub mod reviewer;
pub mod scheduler;

use crate::event::AgentEvent;
use crate::tool::builtin::agent::AgentTool;
use crate::tool::{Tool, ToolContext};
use anyhow::{anyhow, bail, Result};
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

        self.emit(
            "orchestration_plan",
            json!({
                "planId": plan.plan_id,
                "description": plan.description,
                "stages": plan.stages,
            }),
        )
        .await;

        for stage in &plan.stages {
            for dep in &stage.depends_on {
                if results
                    .values()
                    .any(|r| &r.stage_id == dep && r.status != SubAgentStatus::Success)
                {
                    bail!("dependency stage '{}' contains failed agents", dep);
                }
            }

            let stage_results = execute_stage(stage, &cancellation_token, {
                let tool = self.agent_tool.clone();
                let ctx = self.tool_context.clone();
                move |agent_req, stage_id| {
                    let tool = tool.clone();
                    let ctx = ctx.clone();
                    async move {
                        let output = tool
                            .execute(
                                json!({
                                    "prompt": agent_req.prompt,
                                    "description": agent_req.description,
                                    "subagent_type": agent_req.subagent_type,
                                    "run_in_background": false,
                                    "agent_id": agent_req.agent_id,
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
                            agent_id: agent_req.agent_id,
                            stage_id,
                            status: SubAgentStatus::Success,
                            output: content,
                        })
                    }
                }
            })
            .await?;

            let all_success = stage_results.iter().all(|r| r.status == SubAgentStatus::Success);
            self.emit(
                "stage_complete",
                json!({ "stageId": stage.stage_id, "mode": stage.mode.as_str(), "allSuccess": all_success }),
            )
            .await;

            if !all_success {
                bail!("stage '{}' execution failed", stage.stage_id);
            }

            for result in stage_results {
                self.emit(
                    "sub_agent_complete",
                    json!({
                        "agentId": result.agent_id,
                        "stageId": result.stage_id,
                        "status": "success",
                        "outputSummary": result.output,
                    }),
                )
                .await;
                results.insert(result.agent_id.clone(), result);
            }
        }

        self.emit("orchestration_review_start", json!({ "planId": plan.plan_id }))
            .await;
        let review = self.run_review(&plan, &results).await?;

        self.emit(
            "orchestration_complete",
            json!({
                "planId": plan.plan_id,
                "overallSuccess": review.success,
                "summary": review.summary,
            }),
        )
        .await;

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

    async fn emit(&self, kind: &str, args: serde_json::Value) {
        let payload = format!(
            "{{\"kind\":{},\"args\":{}}}",
            serde_json::to_string(kind).unwrap_or_else(|_| "\"unknown\"".to_string()),
            args
        );
        let _ = self
            .event_tx
            .send(AgentEvent::SystemLog(format!("[orchestration] {}", payload)))
            .await;
    }
}
