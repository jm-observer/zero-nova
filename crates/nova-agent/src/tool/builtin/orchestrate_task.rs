use crate::orchestrator::planner::{self, AgentRequest, ExecutionStage, OrchestrationPlan, StageMode};
use crate::orchestrator::{OrchestratorEngine, SubAgentExecutor};
use crate::tool::builtin::agent::AgentTool;
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OrchestrateTaskTool {
    agent_tool: Arc<AgentTool>,
}

impl OrchestrateTaskTool {
    pub fn new(agent_tool: Arc<AgentTool>) -> Self {
        Self { agent_tool }
    }

    pub fn input_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "object",
                    "description": "完整编排计划（多 Agent 模式）。与 prompt 互斥。结构: {planId: string, description: string, skipReview?: boolean, stages: [{stageId: string, mode: \"parallel\"|\"serial\", dependsOn?: [stageId...], agents: [{agentId: string, description: string, prompt: string, agentSelection?: string, outputFormat?: string}]}]}"
                },
                "prompt": {
                    "type": "string",
                    "description": "快捷模式：单 Agent 任务的 prompt。与 plan 互斥"
                },
                "description": {
                    "type": "string",
                    "description": "快捷模式：3-5 词任务描述"
                },
                "agentSelection": {
                    "type": "string",
                    "description": "快捷模式：可选的 agent 类型选择"
                },
                "skill": {
                    "type": "string",
                    "description": "快捷模式：可选 skill slug。设置后子 Agent 的 system prompt 严格等于该 skill 正文（不叠加主 prompt/behavior guards），并预激活该 skill 声明的 preload 工具"
                }
            }
        })
    }
}

fn build_shorthand_plan(
    prompt: &str,
    description: &str,
    agent_selection: Option<String>,
    skill: Option<String>,
) -> OrchestrationPlan {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    OrchestrationPlan {
        plan_id: format!("shorthand-{}", ts),
        description: description.to_string(),
        skip_review: true,
        max_retries: None,
        stages: vec![ExecutionStage {
            stage_id: "s1".to_string(),
            mode: StageMode::Parallel,
            depends_on: vec![],
            agents: vec![AgentRequest {
                agent_id: "a1".to_string(),
                subagent_type: "nova".to_string(),
                agent_selection,
                description: description.to_string(),
                prompt: prompt.to_string(),
                context_files: vec![],
                output_format: Some("full".to_string()),
                skill,
            }],
        }],
    }
}

#[async_trait]
impl Tool for OrchestrateTaskTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "OrchestrateTask".to_string(),
            description: "Execute a multi-agent orchestration plan, or run a single agent task in shorthand mode. IMPORTANT: Call only ONCE per user request. Do NOT retry if the first call returns overallSuccess=true."
                .to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let tool_context = context.context("OrchestrateTask requires tool context")?;

        let cancellation_token = tool_context
            .cancellation_token
            .clone()
            .unwrap_or_else(CancellationToken::new);

        let has_plan = input.get("plan").is_some();
        let has_prompt = input.get("prompt").and_then(Value::as_str).is_some();

        if has_plan && has_prompt {
            anyhow::bail!("'plan' and 'prompt' are mutually exclusive");
        }

        let (plan, is_shorthand) = if let Some(plan_value) = input.get("plan") {
            let raw: OrchestrationPlan = serde_json::from_value(plan_value.clone()).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid plan object: {}. Expected structure: {{planId, description, stages: [{{stageId, mode: \"parallel\"|\"serial\", dependsOn?: [], agents: [{{agentId, description, prompt}}]}}]}}",
                    e
                )
            })?;
            (planner::validate_and_sort(raw)?, false)
        } else if let Some(prompt) = input.get("prompt").and_then(Value::as_str) {
            let desc = input.get("description").and_then(Value::as_str).unwrap_or("Agent task");
            let agent_selection = input.get("agentSelection").and_then(Value::as_str).map(String::from);
            let skill = input.get("skill").and_then(Value::as_str).map(String::from);
            (build_shorthand_plan(prompt, desc, agent_selection, skill), true)
        } else {
            anyhow::bail!("Either 'plan' (object) or 'prompt' (string) must be provided");
        };

        log::info!(
            "[OrchestrateTaskTool] Received orchestration request plan_id={} stage_count={}",
            plan.plan_id,
            plan.stages.len()
        );

        let executor: Arc<dyn SubAgentExecutor> = self.agent_tool.clone();
        let engine = OrchestratorEngine::new(executor, tool_context.event_tx.clone(), Some(tool_context));
        let outcome = engine
            .execute_plan(plan, cancellation_token)
            .await
            .map_err(|e| anyhow::anyhow!("Orchestration execution failed: {:#}", e))?;

        log::info!(
            "[OrchestrateTaskTool] Execution finished plan_id={} stage_count={} result_count={} review_present={}",
            outcome.plan.plan_id,
            outcome.plan.stages.len(),
            outcome.results.len(),
            outcome.review.is_some()
        );

        if is_shorthand {
            let content = outcome
                .results
                .get("a1")
                .map(|result| {
                    if result.output.is_empty() {
                        result.error.clone().unwrap_or_default()
                    } else {
                        result.output.clone()
                    }
                })
                .unwrap_or_else(|| "subagent produced no output".to_string());
            return Ok(ToolOutput {
                content,
                is_error: false,
            });
        }

        let review = outcome.review.as_ref();
        let output = json!({
            "planId": outcome.plan.plan_id,
            "stageCount": outcome.plan.stages.len(),
            "resultCount": outcome.results.len(),
            "overallSuccess": review.map(|item| item.success).unwrap_or(false),
            "summary": review.map(|item| item.summary.clone()).unwrap_or_else(|| "orchestration finished".to_string())
        });

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&output)?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::OrchestrateTaskTool;
    use crate::config::AppConfig;
    use crate::event::AgentEvent;
    use crate::prompt::EnvironmentSnapshot;
    use crate::tool::builtin::agent::AgentTool;
    use crate::tool::{Tool, ToolContext};
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};

    fn test_config() -> AppConfig {
        AppConfig::new("D:/config".into())
    }

    fn test_context(event_tx: mpsc::Sender<AgentEvent>) -> ToolContext {
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
            cancellation_token: None,
            visible_tool_names: Arc::new(HashSet::new()),
        }
    }

    #[tokio::test]
    async fn orchestrate_task_requires_tool_context() {
        let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new_without_subagent_services(test_config())));

        let error = tool
            .execute(
                serde_json::json!({
                    "plan": {"planId": "p1", "description": "d", "stages": []}
                }),
                None,
            )
            .await;

        assert!(error.is_err(), "missing context should fail");
        assert!(error
            .err()
            .expect("error should exist")
            .to_string()
            .contains("requires tool context"));
    }

    #[tokio::test]
    async fn full_plan_mode_accepts_minimal_valid_plan() {
        let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new_without_subagent_services(test_config())));
        let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(4);

        let output = tool
            .execute(
                serde_json::json!({
                    "plan": {"planId": "p1", "description": "d", "stages": []}
                }),
                Some(test_context(event_tx)),
            )
            .await
            .expect("empty plan should succeed without sub-agents");

        assert!(output.content.contains("\"planId\": \"p1\""));
    }

    #[tokio::test]
    async fn shorthand_mode_constructs_single_agent_plan() {
        let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new_without_subagent_services(test_config())));
        let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(64);

        let result = tool
            .execute(
                serde_json::json!({
                    "prompt": "do something",
                    "description": "test task"
                }),
                Some(test_context(event_tx)),
            )
            .await;

        // Without real subagent services the agent execution fails gracefully.
        // Shorthand now returns the sub-agent's output/error text verbatim (not a plan summary).
        let output = result.expect("shorthand plan should not hard-fail");
        assert!(!output.is_error, "shorthand should not hard-error");
        assert!(
            !output.content.is_empty(),
            "shorthand should return sub-agent output/error text, got empty"
        );
        assert!(
            !output.content.contains("\"overallSuccess\""),
            "shorthand should NOT return the plan summary JSON: {}",
            output.content
        );
    }

    #[tokio::test]
    async fn plan_and_prompt_mutually_exclusive() {
        let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new_without_subagent_services(test_config())));
        let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(4);

        let error = tool
            .execute(
                serde_json::json!({
                    "plan": {"planId": "p1", "description": "d", "stages": []},
                    "prompt": "also this"
                }),
                Some(test_context(event_tx)),
            )
            .await;

        assert!(error.is_err());
        assert!(error
            .err()
            .expect("error should exist")
            .to_string()
            .contains("mutually exclusive"));
    }

    #[tokio::test]
    async fn missing_both_plan_and_prompt_fails() {
        let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new_without_subagent_services(test_config())));
        let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(4);

        let error = tool.execute(serde_json::json!({}), Some(test_context(event_tx))).await;

        assert!(error.is_err());
        assert!(error.err().expect("error should exist").to_string().contains("Either"));
    }
}
