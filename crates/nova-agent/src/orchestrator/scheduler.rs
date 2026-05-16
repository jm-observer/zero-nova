use super::planner::{AgentRequest, ExecutionStage, StageMode};
use anyhow::Result;
use log::debug;
use std::future::Future;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Success,
    Failed,
    Cancelled,
}

impl SubAgentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub plan_id: String,
    pub agent_id: String,
    pub stage_id: String,
    pub status: SubAgentStatus,
    pub output: String,
    pub error: Option<String>,
}

pub async fn execute_stage<F, Fut>(
    plan_id: &str,
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    match stage.mode {
        StageMode::Parallel => execute_parallel(plan_id, stage, cancellation_token, execute_agent).await,
        StageMode::Serial => execute_serial(plan_id, stage, cancellation_token, execute_agent).await,
    }
}

async fn execute_parallel<F, Fut>(
    plan_id: &str,
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    let mut join_set = JoinSet::new();
    let mut pending_agents = stage.agents.clone();

    debug!(
        "[scheduler] execute_parallel start plan_id={} stage_id={} agent_count={}",
        plan_id,
        stage.stage_id,
        stage.agents.len()
    );

    for agent in &stage.agents {
        let stage_id = stage.stage_id.clone();
        let req = agent.clone();
        let agent_id = req.agent_id.clone();
        let runner = execute_agent.clone();
        debug!(
            "[scheduler] spawn parallel agent plan_id={} stage_id={} agent_id={}",
            plan_id, stage.stage_id, agent_id
        );
        join_set.spawn(async move { (agent_id, runner(req, stage_id).await) });
    }

    let mut results = Vec::new();
    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                debug!(
                    "[scheduler] cancellation observed plan_id={} stage_id={} pending_agents={}",
                    plan_id,
                    stage.stage_id,
                    pending_agents.len()
                );
                join_set.abort_all();
                break;
            }
            joined = join_set.join_next() => {
                let Some(joined) = joined else {
                    debug!(
                        "[scheduler] execute_parallel drained plan_id={} stage_id={}",
                        plan_id,
                        stage.stage_id
                    );
                    break;
                };
                match joined {
                    Ok((agent_id, Ok(result))) => {
                        debug!(
                            "[scheduler] parallel agent completed plan_id={} stage_id={} agent_id={} status={}",
                            plan_id,
                            stage.stage_id,
                            agent_id,
                            result.status.as_str()
                        );
                        pending_agents.retain(|agent| agent.agent_id != agent_id);
                        results.push(result);
                    }
                    Ok((agent_id, Err(error))) => {
                        debug!(
                            "[scheduler] parallel agent failed plan_id={} stage_id={} agent_id={} error={}",
                            plan_id,
                            stage.stage_id,
                            agent_id,
                            error
                        );
                        if let Some(agent) = pending_agents.iter().find(|item| item.agent_id == agent_id) {
                            results.push(failed_result(plan_id, &stage.stage_id, &agent.agent_id, error.to_string()));
                        }
                        pending_agents.retain(|agent| agent.agent_id != agent_id);
                    }
                    Err(error) => {
                        debug!(
                            "[scheduler] parallel join failure plan_id={} stage_id={} error={}",
                            plan_id,
                            stage.stage_id,
                            error
                        );
                        results.push(failed_result(
                            plan_id,
                            &stage.stage_id,
                            "unknown-agent",
                            format!("sub-agent task join failure: {error}"),
                        ));
                    }
                }
            }
        }
    }

    if cancellation_token.is_cancelled() {
        for agent in pending_agents {
            debug!(
                "[scheduler] mark cancelled plan_id={} stage_id={} agent_id={}",
                plan_id, stage.stage_id, agent.agent_id
            );
            results.push(cancelled_result(plan_id, &stage.stage_id, &agent.agent_id));
        }
    }

    debug!(
        "[scheduler] execute_parallel end plan_id={} stage_id={} result_count={}",
        plan_id,
        stage.stage_id,
        results.len()
    );

    Ok(results)
}

async fn execute_serial<F, Fut>(
    plan_id: &str,
    stage: &ExecutionStage,
    cancellation_token: &CancellationToken,
    execute_agent: F,
) -> Result<Vec<SubAgentResult>>
where
    F: Fn(AgentRequest, String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<SubAgentResult>> + Send + 'static,
{
    let mut results = Vec::new();
    debug!(
        "[scheduler] execute_serial start plan_id={} stage_id={} agent_count={}",
        plan_id,
        stage.stage_id,
        stage.agents.len()
    );
    for (index, agent) in stage.agents.iter().enumerate() {
        if cancellation_token.is_cancelled() {
            debug!(
                "[scheduler] serial cancellation observed plan_id={} stage_id={} remaining_agents={}",
                plan_id,
                stage.stage_id,
                stage.agents.len().saturating_sub(index)
            );
            append_cancelled_results(plan_id, &stage.stage_id, &stage.agents[index..], &mut results);
            break;
        }
        debug!(
            "[scheduler] serial agent start plan_id={} stage_id={} agent_id={}",
            plan_id, stage.stage_id, agent.agent_id
        );
        match execute_agent.clone()(agent.clone(), stage.stage_id.clone()).await {
            Ok(result) => {
                let failed = result.status == SubAgentStatus::Failed;
                let cancelled = result.status == SubAgentStatus::Cancelled;
                debug!(
                    "[scheduler] serial agent completed plan_id={} stage_id={} agent_id={} status={}",
                    plan_id,
                    stage.stage_id,
                    result.agent_id,
                    result.status.as_str()
                );
                results.push(result);
                if failed || cancelled {
                    debug!(
                        "[scheduler] serial short-circuit plan_id={} stage_id={} remaining_agents={}",
                        plan_id,
                        stage.stage_id,
                        stage.agents.len().saturating_sub(index + 1)
                    );
                    append_cancelled_results(plan_id, &stage.stage_id, &stage.agents[index + 1..], &mut results);
                    break;
                }
            }
            Err(error) => {
                debug!(
                    "[scheduler] serial agent failed plan_id={} stage_id={} agent_id={} error={}",
                    plan_id, stage.stage_id, agent.agent_id, error
                );
                results.push(failed_result(
                    plan_id,
                    &stage.stage_id,
                    &agent.agent_id,
                    error.to_string(),
                ));
                append_cancelled_results(plan_id, &stage.stage_id, &stage.agents[index + 1..], &mut results);
                break;
            }
        }
    }
    debug!(
        "[scheduler] execute_serial end plan_id={} stage_id={} result_count={}",
        plan_id,
        stage.stage_id,
        results.len()
    );
    Ok(results)
}

fn append_cancelled_results(plan_id: &str, stage_id: &str, agents: &[AgentRequest], results: &mut Vec<SubAgentResult>) {
    results.extend(
        agents
            .iter()
            .map(|agent| cancelled_result(plan_id, stage_id, &agent.agent_id)),
    );
}

fn failed_result(plan_id: &str, stage_id: &str, agent_id: &str, error: String) -> SubAgentResult {
    SubAgentResult {
        plan_id: plan_id.to_string(),
        agent_id: agent_id.to_string(),
        stage_id: stage_id.to_string(),
        status: SubAgentStatus::Failed,
        output: String::new(),
        error: Some(error),
    }
}

fn cancelled_result(plan_id: &str, stage_id: &str, agent_id: &str) -> SubAgentResult {
    SubAgentResult {
        plan_id: plan_id.to_string(),
        agent_id: agent_id.to_string(),
        stage_id: stage_id.to_string(),
        status: SubAgentStatus::Cancelled,
        output: String::new(),
        error: Some("cancelled".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{execute_stage, SubAgentResult, SubAgentStatus};
    use crate::orchestrator::planner::{AgentRequest, ExecutionStage, StageMode};
    use anyhow::anyhow;
    use custom_utils::logger::logger_feature;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Once};
    use tokio_util::sync::CancellationToken;

    fn init_test_logger() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = logger_feature("nova-agent-test", "debug", log::LevelFilter::Debug, false).build();
        });
    }

    fn build_stage(mode: StageMode) -> ExecutionStage {
        ExecutionStage {
            stage_id: "stage-1".to_string(),
            mode,
            depends_on: vec![],
            agents: vec![
                AgentRequest {
                    agent_id: "a1".to_string(),
                    subagent_type: "Coder".to_string(),
                    agent_selection: None,
                    description: "first".to_string(),
                    prompt: "p1".to_string(),
                    context_files: vec![],
                    output_format: None,
                },
                AgentRequest {
                    agent_id: "a2".to_string(),
                    subagent_type: "Coder".to_string(),
                    agent_selection: None,
                    description: "second".to_string(),
                    prompt: "p2".to_string(),
                    context_files: vec![],
                    output_format: None,
                },
            ],
        }
    }

    fn build_empty_stage(mode: StageMode) -> ExecutionStage {
        ExecutionStage {
            stage_id: "stage-1".to_string(),
            mode,
            depends_on: vec![],
            agents: vec![],
        }
    }

    fn build_single_agent_stage(mode: StageMode) -> ExecutionStage {
        ExecutionStage {
            stage_id: "stage-1".to_string(),
            mode,
            depends_on: vec![],
            agents: vec![AgentRequest {
                agent_id: "a1".to_string(),
                subagent_type: "Coder".to_string(),
                agent_selection: None,
                description: "first".to_string(),
                prompt: "p1".to_string(),
                context_files: vec![],
                output_format: None,
            }],
        }
    }

    fn success_result(plan_id: &str, stage_id: &str, agent_id: &str, output: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: plan_id.to_string(),
            agent_id: agent_id.to_string(),
            stage_id: stage_id.to_string(),
            status: SubAgentStatus::Success,
            output: output.to_string(),
            error: None,
        }
    }

    fn failed_result_with_error(plan_id: &str, stage_id: &str, agent_id: &str, error: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: plan_id.to_string(),
            agent_id: agent_id.to_string(),
            stage_id: stage_id.to_string(),
            status: SubAgentStatus::Failed,
            output: String::new(),
            error: Some(error.to_string()),
        }
    }

    fn cancelled_result(plan_id: &str, stage_id: &str, agent_id: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: plan_id.to_string(),
            agent_id: agent_id.to_string(),
            stage_id: stage_id.to_string(),
            status: SubAgentStatus::Cancelled,
            output: String::new(),
            error: Some("cancelled".to_string()),
        }
    }

    #[tokio::test]
    async fn parallel_all_success() {
        init_test_logger();
        let stage = build_stage(StageMode::Parallel);

        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move { Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done")) },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|item| item.status == SubAgentStatus::Success));
    }

    #[tokio::test]
    async fn serial_all_success() {
        init_test_logger();
        let stage = build_stage(StageMode::Serial);
        let order = Arc::new(AtomicUsize::new(0));

        let results = execute_stage("plan-1", &stage, &CancellationToken::new(), {
            let order = order.clone();
            move |agent, stage_id| {
                let current = order.fetch_add(1, Ordering::SeqCst);
                async move {
                    if agent.agent_id == "a1" {
                        assert_eq!(current, 0);
                    } else {
                        assert_eq!(current, 1);
                    }
                    Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
                }
            }
        })
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|item| item.status == SubAgentStatus::Success));
    }

    #[tokio::test]
    async fn serial_error_path() {
        init_test_logger();
        let stage = build_stage(StageMode::Serial);

        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move {
                if agent.agent_id == "a1" {
                    return Err(anyhow!("error msg"));
                }
                Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
            },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results[0].status, SubAgentStatus::Failed);
        assert!(results[0]
            .error
            .as_deref()
            .expect("error should exist")
            .contains("error msg"));
        assert_eq!(results[1].status, SubAgentStatus::Cancelled);
    }

    #[tokio::test]
    async fn serial_cancellation_mid_execution() {
        init_test_logger();
        let stage = build_stage(StageMode::Serial);
        let token = CancellationToken::new();
        let token_for_closure = token.clone();

        let results = execute_stage("plan-1", &stage, &token, move |agent, stage_id| {
            let token_for_closure = token_for_closure.clone();
            async move {
                if agent.agent_id == "a1" {
                    token_for_closure.cancel();
                }
                Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
            }
        })
        .await
        .expect("stage should succeed");

        assert_eq!(results[0].status, SubAgentStatus::Success);
        assert_eq!(results[1].status, SubAgentStatus::Cancelled);
    }

    #[tokio::test]
    async fn empty_stage_zero_agents() {
        init_test_logger();
        let stage = build_empty_stage(StageMode::Parallel);

        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |_agent, _stage_id| async move { Ok(success_result("plan-1", "stage-1", "unused", "done")) },
        )
        .await
        .expect("stage should succeed");

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn single_agent_parallel() {
        init_test_logger();
        let stage = build_single_agent_stage(StageMode::Parallel);

        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move { Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done")) },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SubAgentStatus::Success);
    }

    #[tokio::test]
    async fn single_agent_serial() {
        init_test_logger();
        let stage = build_single_agent_stage(StageMode::Serial);

        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move { Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done")) },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SubAgentStatus::Success);
    }

    #[tokio::test]
    async fn parallel_stage_preserves_failure_results() {
        init_test_logger();
        let stage = build_stage(StageMode::Parallel);
        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move {
                if agent.agent_id == "a2" {
                    return Err(anyhow!("boom"));
                }
                Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
            },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .any(|item| item.agent_id == "a2" && item.status == SubAgentStatus::Failed));
    }

    #[tokio::test]
    async fn serial_stage_cancels_remaining_agents_after_failure() {
        init_test_logger();
        let stage = build_stage(StageMode::Serial);
        let results = execute_stage(
            "plan-1",
            &stage,
            &CancellationToken::new(),
            |agent, stage_id| async move {
                if agent.agent_id == "a1" {
                    return Ok(failed_result_with_error("plan-1", &stage_id, &agent.agent_id, "failed"));
                }
                Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
            },
        )
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, SubAgentStatus::Failed);
        assert_eq!(results[1].status, SubAgentStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancellation_marks_remaining_parallel_agents_cancelled() {
        init_test_logger();
        let stage = build_stage(StageMode::Parallel);
        let token = CancellationToken::new();
        token.cancel();

        let results = execute_stage("plan-1", &stage, &token, |agent, stage_id| async move {
            Ok(success_result("plan-1", &stage_id, &agent.agent_id, "done"))
        })
        .await
        .expect("stage should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, cancelled_result("plan-1", "stage-1", "a1").status);
        assert_eq!(results[0].agent_id, "a1");
        assert_eq!(results[1].status, cancelled_result("plan-1", "stage-1", "a2").status);
        assert_eq!(results[1].agent_id, "a2");
    }
}
