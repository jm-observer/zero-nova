use super::planner::{AgentRequest, ExecutionStage, StageMode};
use anyhow::Result;
use std::future::Future;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub agent_id: String,
    pub stage_id: String,
    pub status: SubAgentStatus,
    pub output: String,
}

pub async fn execute_stage<F, Fut>(
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    match stage.mode {
        StageMode::Parallel => execute_parallel(stage, cancellation_token, execute_agent).await,
        StageMode::Serial => execute_serial(stage, cancellation_token, execute_agent).await,
    }
}

async fn execute_parallel<F, Fut>(
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    let mut join_set = JoinSet::new();

    for agent in &stage.agents {
        let stage_id = stage.stage_id.clone();
        let req = agent.clone();
        let runner = execute_agent.clone();
        join_set.spawn(async move { runner(req, stage_id).await });
    }

    let mut results = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        if cancellation_token.is_cancelled() {
            return Ok(results);
        }
        let res = joined??;
        results.push(res);
    }

    Ok(results)
}

async fn execute_serial<F, Fut>(
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    let mut results = Vec::new();
    for agent in &stage.agents {
        if cancellation_token.is_cancelled() {
            break;
        }
        let res = execute_agent.clone()(agent.clone(), stage.stage_id.clone()).await?;
        results.push(res);
    }
    Ok(results)
}
