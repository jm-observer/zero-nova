use crate::config::AppConfig;
use crate::orchestrator::OrchestratorEngine;
use crate::tool::builtin::agent::AgentTool;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OrchestrateTaskTool {
    config: AppConfig,
}

impl OrchestrateTaskTool {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub fn input_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "planJson": {
                    "type": "string",
                    "description": "符合 OrchestrationPlan 协议的 JSON 字符串"
                }
            },
            "required": ["planJson"]
        })
    }
}

#[async_trait]
impl Tool for OrchestrateTaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "OrchestrateTask".to_string(),
            description: "Execute a validated multi-agent orchestration plan.".to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let plan_json = input
            .get("planJson")
            .and_then(Value::as_str)
            .context("Missing 'planJson'")?;
        let tool_context = context.context("OrchestrateTask requires tool context")?;

        let engine = OrchestratorEngine::new(
            Arc::new(AgentTool::new(self.config.clone())),
            tool_context.event_tx.clone(),
            Some(tool_context),
        );
        let outcome = engine
            .execute_plan(plan_json, CancellationToken::new())
            .await
            .context("Failed to execute orchestration plan")?;

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
    use crate::config::{AppConfig, OriginAppConfig};
    use crate::event::AgentEvent;
    use crate::prompt::EnvironmentSnapshot;
    use crate::tool::{Tool, ToolContext};
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};

    fn test_config() -> AppConfig {
        AppConfig::from_origin(OriginAppConfig::default(), "D:/config".into())
    }

    #[tokio::test]
    async fn orchestrate_task_requires_tool_context() {
        let tool = OrchestrateTaskTool::new(test_config());

        let error = tool
            .execute(
                serde_json::json!({
                    "planJson": "{\"planId\":\"p1\",\"description\":\"d\",\"stages\":[]}"
                }),
                None,
            )
            .await;

        assert!(error.is_err(), "missing context should fail");
        let error = error.err().expect("error should exist");

        assert!(error.to_string().contains("requires tool context"));
    }

    #[tokio::test]
    async fn orchestrate_task_accepts_minimal_valid_plan() {
        let tool = OrchestrateTaskTool::new(test_config());
        let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(4);

        let output = tool
            .execute(
                serde_json::json!({
                    "planJson": "{\"planId\":\"p1\",\"description\":\"d\",\"stages\":[]}"
                }),
                Some(ToolContext {
                    event_tx,
                    tool_use_id: "tool-1".to_string(),
                    session_id: "session-1".to_string(),
                    task_store: None,
                    skill_registry: None,
                    read_files: Arc::new(Mutex::new(HashSet::new())),
                    environment: Some(EnvironmentSnapshot {
                        config_dir: "D:/config".to_string(),
                        project_dir: None,
                        platform: "windows".to_string(),
                        shell: "powershell".to_string(),
                        git_branch: None,
                        git_status_summary: None,
                        recent_commits: None,
                        model_id: None,
                        current_date: "2026-05-06".to_string(),
                    }),
                    shared_environment: None,
                }),
            )
            .await
            .expect("empty plan should succeed without sub-agents");

        assert!(output.content.contains("\"planId\": \"p1\""));
    }
}
