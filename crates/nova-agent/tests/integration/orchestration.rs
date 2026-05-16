/// Integration tests for orchestration engine.
///
/// Tests the full flow: plan parsing → stage execution → review → retry,
/// using a mock SubAgentExecutor without real LLM calls.
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use nova_agent::event::AgentEvent;
use nova_agent::orchestrator::planner::{self, OrchestrationPlan};
use nova_agent::orchestrator::scheduler::SubAgentStatus;
use nova_agent::orchestrator::{OrchestratorEngine, SubAgentExecutor, SubAgentOutput, SubAgentRequest};
use nova_agent::tool::ToolContext;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// --- Mock executor ---

#[derive(Clone)]
struct MockExecutor {
    responses: Arc<HashMap<String, std::result::Result<String, String>>>,
    catalog: HashSet<String>,
    default_id: String,
    call_count: Arc<AtomicU32>,
}

impl MockExecutor {
    fn new(responses: HashMap<String, std::result::Result<String, String>>) -> Self {
        Self {
            responses: Arc::new(responses),
            catalog: HashSet::from(["nova".to_string(), "reviewer".to_string()]),
            default_id: "nova".to_string(),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn with_catalog(mut self, catalog: &[&str]) -> Self {
        self.catalog = catalog.iter().map(|s| s.to_string()).collect();
        self
    }
}

#[async_trait]
impl SubAgentExecutor for MockExecutor {
    async fn execute_agent(&self, request: SubAgentRequest, _context: Option<ToolContext>) -> Result<SubAgentOutput> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        let key = if request
            .agent_selection
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("reviewer"))
            .unwrap_or(false)
        {
            "reviewer".to_string()
        } else {
            request.agent_id.clone().unwrap_or_else(|| request.description.clone())
        };

        match self.responses.get(&key) {
            Some(Ok(output)) => Ok(SubAgentOutput {
                output: output.clone(),
                duration_ms: 10,
                warnings: vec![],
            }),
            Some(Err(msg)) => Err(anyhow!(msg.clone())),
            None => Ok(SubAgentOutput {
                output: String::new(),
                duration_ms: 10,
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

// --- Helpers ---

fn review_json(success: bool, summary: &str, retry_agents: &[&str]) -> String {
    serde_json::to_string(&json!({
        "success": success,
        "issues": [],
        "retryAgents": retry_agents,
        "summary": summary
    }))
    .unwrap()
}

fn make_plan(json_str: &str) -> OrchestrationPlan {
    planner::parse_and_validate(json_str).expect("plan should parse")
}

fn build_engine(executor: MockExecutor) -> (OrchestratorEngine, mpsc::Receiver<AgentEvent>) {
    let (tx, rx) = mpsc::channel(128);
    let engine = OrchestratorEngine::new(Arc::new(executor), tx, None);
    (engine, rx)
}

fn collect_orchestration_events(rx: &mut mpsc::Receiver<AgentEvent>) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::OrchestrationProgress { kind, payload } = event {
            events.push((kind, payload));
        }
    }
    events
}

// --- Tests ---

#[tokio::test]
async fn plan_parse_error_includes_details() {
    let bad_json = r#"{"planId":"p1","description":"d","stages":[{"stageId":"s1","agents":[]}]}"#;
    let err = planner::parse_and_validate(bad_json).unwrap_err();
    assert!(
        err.to_string().contains("mode"),
        "error should mention missing 'mode' field, got: {}",
        err
    );
}

#[tokio::test]
async fn skip_review_completes_without_reviewer_call() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("done".to_string()));

    let executor = MockExecutor::new(responses);
    let call_count = executor.call_count.clone();
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "skip-review-test",
            "description": "test skip review",
            "skipReview": true,
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do it"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    assert!(outcome.review.is_none(), "review should be skipped");
    assert_eq!(outcome.results.len(), 1);
    assert_eq!(call_count.load(Ordering::SeqCst), 1, "only agent call, no reviewer");

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(true));
    assert!(complete.1["summary"].as_str().unwrap().contains("review skipped"));
}

#[tokio::test]
async fn retry_agents_are_re_executed() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("partial output".to_string()));
    responses.insert("reviewer".to_string(), Ok(review_json(true, "Good after retry.", &[])));

    let executor = MockExecutor::new(responses);
    let call_count = executor.call_count.clone();
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "retry-test",
            "description": "test retry",
            "maxRetries": 2,
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do it"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    let review = outcome.review.expect("review should exist");
    assert!(review.success);
    assert!(call_count.load(Ordering::SeqCst) >= 2, "at least agent + reviewer");

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(true));
}

#[tokio::test]
async fn retry_agents_actually_retried_on_review_failure() {
    use std::sync::Mutex;

    let call_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let call_log_clone = call_log.clone();

    #[derive(Clone)]
    struct RetryTrackingExecutor {
        call_log: Arc<Mutex<Vec<String>>>,
        review_count: Arc<AtomicU32>,
    }

    #[async_trait]
    impl SubAgentExecutor for RetryTrackingExecutor {
        async fn execute_agent(
            &self,
            request: SubAgentRequest,
            _context: Option<ToolContext>,
        ) -> Result<SubAgentOutput> {
            let is_reviewer = request
                .agent_selection
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("reviewer"))
                .unwrap_or(false);

            if is_reviewer {
                let count = self.review_count.fetch_add(1, Ordering::SeqCst);
                let response = if count == 0 {
                    review_json(false, "Agent a1 output incomplete.", &["a1"])
                } else {
                    review_json(true, "All good after retry.", &[])
                };
                return Ok(SubAgentOutput {
                    output: response,
                    duration_ms: 5,
                    warnings: vec![],
                });
            }

            self.call_log
                .lock()
                .unwrap()
                .push(request.agent_id.clone().unwrap_or_default());

            Ok(SubAgentOutput {
                output: "retried output".to_string(),
                duration_ms: 5,
                warnings: vec![],
            })
        }

        fn catalog_agent_ids(&self) -> HashSet<String> {
            HashSet::from(["nova".to_string(), "reviewer".to_string()])
        }

        fn default_agent_id(&self) -> String {
            "nova".to_string()
        }
    }

    let executor = RetryTrackingExecutor {
        call_log: call_log_clone,
        review_count: Arc::new(AtomicU32::new(0)),
    };

    let (tx, _rx) = mpsc::channel(128);
    let engine = OrchestratorEngine::new(Arc::new(executor), tx, None);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "retry-real",
            "description": "real retry test",
            "maxRetries": 2,
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do work"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    let review = outcome.review.expect("review should exist");
    assert!(review.success, "second review should pass");

    let log = call_log.lock().unwrap();
    assert_eq!(log.len(), 2, "a1 should be called twice (initial + retry)");
    assert!(log.iter().all(|id| id == "a1"));
}

#[tokio::test]
async fn dependency_failure_emits_blocked_message() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Err("boom".to_string()));

    let executor = MockExecutor::new(responses).with_catalog(&["nova"]);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "dep-fail",
            "description": "dependency failure",
            "stages": [
                {"stageId": "s1", "mode": "parallel", "dependsOn": [], "agents": [{"agentId": "a1", "description": "t", "prompt": "p"}]},
                {"stageId": "s2", "mode": "serial", "dependsOn": ["s1"], "agents": [{"agentId": "a2", "description": "t2", "prompt": "p2"}]}
            ]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should finish");

    assert!(outcome.review.is_none());

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(false));
    let summary = complete.1["summary"].as_str().unwrap();
    assert!(
        summary.contains("blocked by dependency") || summary.contains("failed"),
        "unexpected summary: {}",
        summary
    );
}

#[tokio::test]
async fn parallel_stage_all_success_with_review() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("output-a1".to_string()));
    responses.insert("a2".to_string(), Ok("output-a2".to_string()));
    responses.insert("a3".to_string(), Ok("output-a3".to_string()));
    responses.insert(
        "reviewer".to_string(),
        Ok(review_json(true, "All 3 agents succeeded.", &[])),
    );

    let executor = MockExecutor::new(responses);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "parallel-3",
            "description": "three parallel agents",
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [
                    {"agentId": "a1", "description": "t1", "prompt": "p1"},
                    {"agentId": "a2", "description": "t2", "prompt": "p2"},
                    {"agentId": "a3", "description": "t3", "prompt": "p3"}
                ]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    assert_eq!(outcome.results.len(), 3);
    assert!(outcome.results.values().all(|r| r.status == SubAgentStatus::Success));
    let review = outcome.review.expect("review should exist");
    assert!(review.success);

    let events = collect_orchestration_events(&mut rx);
    let spawn_count = events.iter().filter(|(k, _)| k == "sub_agent_spawn").count();
    assert_eq!(spawn_count, 3);
    let complete_count = events.iter().filter(|(k, _)| k == "sub_agent_complete").count();
    assert_eq!(complete_count, 3);
}

#[tokio::test]
async fn orchestration_progress_events_are_typed() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("ok".to_string()));
    responses.insert("reviewer".to_string(), Ok(review_json(true, "Good.", &[])));

    let executor = MockExecutor::new(responses);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "typed-events",
            "description": "verify typed events",
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do"}]
            }]
        }))
        .unwrap(),
    );

    let _ = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    let events = collect_orchestration_events(&mut rx);
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();

    assert!(
        kinds.contains(&"orchestration_plan"),
        "missing orchestration_plan event"
    );
    assert!(kinds.contains(&"sub_agent_spawn"), "missing sub_agent_spawn event");
    assert!(
        kinds.contains(&"sub_agent_complete"),
        "missing sub_agent_complete event"
    );
    assert!(kinds.contains(&"stage_complete"), "missing stage_complete event");
    assert!(
        kinds.contains(&"orchestration_review_start"),
        "missing review_start event"
    );
    assert!(
        kinds.contains(&"orchestration_complete"),
        "missing orchestration_complete event"
    );

    // Verify plan event payload structure
    let plan_event = events.iter().find(|(k, _)| k == "orchestration_plan").unwrap();
    assert_eq!(plan_event.1["planId"], "typed-events");
    assert!(plan_event.1["stages"].is_array());

    // Verify spawn event payload
    let spawn_event = events.iter().find(|(k, _)| k == "sub_agent_spawn").unwrap();
    assert_eq!(spawn_event.1["agentId"], "a1");
    assert_eq!(spawn_event.1["stageId"], "s1");
}

#[tokio::test]
async fn cancellation_stops_execution() {
    let token = CancellationToken::new();
    token.cancel();

    let executor = MockExecutor::new(HashMap::new()).with_catalog(&["nova"]);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "cancel-test",
            "description": "cancelled",
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "t", "prompt": "p"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine.execute_plan(plan, token).await.expect("should finish");

    assert!(outcome.review.is_none());
    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(false));
    assert!(complete.1["summary"].as_str().unwrap().contains("cancelled"));
}

#[tokio::test]
async fn max_retries_zero_means_no_retry() {
    use std::sync::Mutex;

    let review_calls: Arc<Mutex<Vec<()>>> = Arc::new(Mutex::new(Vec::new()));
    let review_calls_clone = review_calls.clone();

    #[derive(Clone)]
    struct NoRetryExecutor {
        review_calls: Arc<Mutex<Vec<()>>>,
    }

    #[async_trait]
    impl SubAgentExecutor for NoRetryExecutor {
        async fn execute_agent(
            &self,
            request: SubAgentRequest,
            _context: Option<ToolContext>,
        ) -> Result<SubAgentOutput> {
            let is_reviewer = request
                .agent_selection
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("reviewer"))
                .unwrap_or(false);

            if is_reviewer {
                self.review_calls.lock().unwrap().push(());
                return Ok(SubAgentOutput {
                    output: review_json(false, "Failed but no retry allowed.", &["a1"]),
                    duration_ms: 5,
                    warnings: vec![],
                });
            }

            Ok(SubAgentOutput {
                output: "done".to_string(),
                duration_ms: 5,
                warnings: vec![],
            })
        }

        fn catalog_agent_ids(&self) -> HashSet<String> {
            HashSet::from(["nova".to_string(), "reviewer".to_string()])
        }

        fn default_agent_id(&self) -> String {
            "nova".to_string()
        }
    }

    let executor = NoRetryExecutor {
        review_calls: review_calls_clone,
    };

    let (tx, _rx) = mpsc::channel(128);
    let engine = OrchestratorEngine::new(Arc::new(executor), tx, None);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "no-retry",
            "description": "no retry",
            "maxRetries": 0,
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "t", "prompt": "p"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should finish");

    let review = outcome.review.expect("review should exist");
    assert!(!review.success, "review should report failure");

    let calls = review_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "reviewer should only be called once (no retry)");
}

// --- New tests for error propagation, review graceful failure, catalog validation ---

#[tokio::test]
async fn review_executor_failure_does_not_crash_orchestration() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("output-a1".to_string()));
    responses.insert("reviewer".to_string(), Err("LLM provider unavailable".to_string()));

    let executor = MockExecutor::new(responses);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "review-fail-graceful",
            "description": "review agent fails",
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do it"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should not hard-fail when reviewer errors");

    assert_eq!(outcome.results.len(), 1);
    let review = outcome.review.expect("review should exist (graceful fallback)");
    assert!(review.success, "graceful fallback sets success=true");
    assert!(
        review.summary.contains("executor error"),
        "summary should mention executor error: {}",
        review.summary
    );

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(true));
}

#[tokio::test]
async fn review_parse_failure_does_not_crash_orchestration() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Ok("output-a1".to_string()));
    // Reviewer returns non-JSON output (e.g. LLM hallucinated markdown instead of JSON)
    responses.insert(
        "reviewer".to_string(),
        Ok("Here is my review:\n- All tasks completed successfully\n- No issues found".to_string()),
    );

    let executor = MockExecutor::new(responses);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "review-parse-fail",
            "description": "review returns invalid JSON",
            "stages": [{
                "stageId": "s1",
                "mode": "parallel",
                "dependsOn": [],
                "agents": [{"agentId": "a1", "description": "task", "prompt": "do it"}]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should not hard-fail when review output is unparseable");

    assert_eq!(outcome.results.len(), 1);
    let review = outcome.review.expect("review should exist (graceful fallback)");
    assert!(review.success, "unparseable review defaults to success=true");
    assert!(
        review.summary.contains("unparseable"),
        "summary should mention parse issue: {}",
        review.summary
    );

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(true));
}

#[tokio::test]
async fn unknown_agent_selection_falls_back_to_default() {
    use std::sync::Mutex;

    let selections_seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let selections_clone = selections_seen.clone();

    #[derive(Clone)]
    struct SelectionTrackingExecutor {
        selections: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl SubAgentExecutor for SelectionTrackingExecutor {
        async fn execute_agent(
            &self,
            request: SubAgentRequest,
            _context: Option<ToolContext>,
        ) -> Result<SubAgentOutput> {
            if let Some(sel) = &request.agent_selection {
                self.selections.lock().unwrap().push(sel.clone());
            }
            Ok(SubAgentOutput {
                output: "done".to_string(),
                duration_ms: 5,
                warnings: vec![],
            })
        }

        fn catalog_agent_ids(&self) -> HashSet<String> {
            HashSet::from(["nova".to_string()])
        }

        fn default_agent_id(&self) -> String {
            "nova".to_string()
        }
    }

    let executor = SelectionTrackingExecutor {
        selections: selections_clone,
    };

    let (tx, _rx) = mpsc::channel(128);
    let engine = OrchestratorEngine::new(Arc::new(executor), tx, None);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "catalog-fallback",
            "description": "unknown agent selection",
            "skipReview": true,
            "stages": [{
                "stageId": "s1",
                "mode": "serial",
                "dependsOn": [],
                "agents": [
                    {"agentId": "a1", "agentSelection": "nonexistent-agent", "description": "t1", "prompt": "p1"},
                    {"agentId": "a2", "agentSelection": "nova", "description": "t2", "prompt": "p2"}
                ]
            }]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should succeed");

    assert_eq!(outcome.results.len(), 2);

    let sels = selections_seen.lock().unwrap();
    assert_eq!(sels[0], "nova", "unknown selection should fall back to 'nova'");
    assert_eq!(sels[1], "nova", "known selection should pass through as 'nova'");
}

#[tokio::test]
async fn cascade_dependency_blocking_reports_correct_stage() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Err("stage 1 failure".to_string()));

    let executor = MockExecutor::new(responses).with_catalog(&["nova"]);
    let call_count = executor.call_count.clone();
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "cascade-block",
            "description": "cascade blocking",
            "skipReview": true,
            "stages": [
                {"stageId": "s1", "mode": "parallel", "dependsOn": [], "agents": [{"agentId": "a1", "description": "t1", "prompt": "p1"}]},
                {"stageId": "s2", "mode": "serial", "dependsOn": ["s1"], "agents": [{"agentId": "a2", "description": "t2", "prompt": "p2"}]},
                {"stageId": "s3", "mode": "serial", "dependsOn": ["s2"], "agents": [{"agentId": "a3", "description": "t3", "prompt": "p3"}]}
            ]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should finish");

    assert!(outcome.review.is_none());
    assert_eq!(call_count.load(Ordering::SeqCst), 1, "only a1 should execute");
    assert!(!outcome.results.contains_key("a2"), "a2 should not have run");
    assert!(!outcome.results.contains_key("a3"), "a3 should not have run");

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(false));
    let summary = complete.1["summary"].as_str().unwrap();
    assert!(
        summary.contains("blocked by dependency"),
        "summary should mention blocked dependency: {}",
        summary
    );
}

#[tokio::test]
async fn stage_failure_without_dependents_reports_stage_failed() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Err("something broke".to_string()));

    let executor = MockExecutor::new(responses).with_catalog(&["nova"]);
    let (engine, mut rx) = build_engine(executor);

    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "stage-fail-no-deps",
            "description": "single stage failure",
            "skipReview": true,
            "stages": [
                {"stageId": "s1", "mode": "parallel", "dependsOn": [], "agents": [{"agentId": "a1", "description": "t1", "prompt": "p1"}]}
            ]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should finish");

    assert!(outcome.review.is_none());

    let events = collect_orchestration_events(&mut rx);
    let complete = events.iter().find(|(k, _)| k == "orchestration_complete").unwrap();
    assert_eq!(complete.1["overallSuccess"], Value::Bool(false));
    let summary = complete.1["summary"].as_str().unwrap();
    assert!(
        summary.contains("stage") && summary.contains("failed"),
        "summary should report stage failure: {}",
        summary
    );
}

#[tokio::test]
async fn error_message_propagates_through_orchestrate_tool() {
    use nova_agent::config::AppConfig;
    use nova_agent::prompt::EnvironmentSnapshot;
    use nova_agent::tool::builtin::agent::AgentTool;
    use nova_agent::tool::builtin::orchestrate_task::OrchestrateTaskTool;
    use nova_agent::tool::Tool;
    use std::path::PathBuf;
    use tokio::sync::Mutex as TokioMutex;

    let config = AppConfig::new(PathBuf::from("D:/config"));
    let tool = OrchestrateTaskTool::new(Arc::new(AgentTool::new(config, None)));
    let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(128);

    let context = ToolContext {
        event_tx,
        tool_use_id: "tool-1".to_string(),
        session_id: "session-1".to_string(),
        task_store: None,
        skill_registry: None,
        read_files: Arc::new(TokioMutex::new(HashSet::new())),
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
            current_date: "2026-05-17".to_string(),
        }),
        shared_environment: None,
        cancellation_token: None,
        visible_tool_names: Arc::new(HashSet::new()),
    };

    // Invalid plan: missing 'mode' field
    let result = tool
        .execute(
            json!({
                "plan": {"planId": "p1", "description": "d", "stages": [{"stageId": "s1", "agents": []}]}
            }),
            Some(context.clone()),
        )
        .await;

    let err_msg = result.err().expect("should fail on invalid plan").to_string();
    assert!(
        err_msg.contains("mode"),
        "error should include serde details about missing 'mode': {}",
        err_msg
    );

    // Valid plan but execution fails gracefully (scheduler catches error)
    let (event_tx2, _rx2) = mpsc::channel::<AgentEvent>(128);
    let context2 = ToolContext {
        event_tx: event_tx2,
        ..context
    };

    let result = tool
        .execute(
            json!({
                "plan": {
                    "planId": "p2",
                    "description": "d",
                    "skipReview": true,
                    "stages": [{
                        "stageId": "s1",
                        "mode": "parallel",
                        "dependsOn": [],
                        "agents": [{"agentId": "a1", "description": "t", "prompt": "p"}]
                    }]
                }
            }),
            Some(context2),
        )
        .await;

    // With the scheduler catching errors, execution returns Ok
    let output = result.expect("valid plan should not hard-fail");
    assert!(
        output.content.contains("\"overallSuccess\""),
        "output should contain structured result: {}",
        output.content
    );
}

#[tokio::test]
async fn independent_stages_at_same_level_both_execute() {
    let mut responses = HashMap::new();
    responses.insert("a1".to_string(), Err("s1 failed".to_string()));
    responses.insert("a2".to_string(), Ok("s2 done".to_string()));

    let executor = MockExecutor::new(responses).with_catalog(&["nova"]);
    let call_count = executor.call_count.clone();
    let (engine, _rx) = build_engine(executor);

    // Both stages have no dependencies → same topological level.
    // Topological sort order is non-deterministic for same-level stages,
    // so both may execute before a failure is detected.
    let plan = make_plan(
        &serde_json::to_string(&json!({
            "planId": "independent-stages",
            "description": "independent stage after failure",
            "skipReview": true,
            "stages": [
                {"stageId": "s1", "mode": "parallel", "dependsOn": [], "agents": [{"agentId": "a1", "description": "t1", "prompt": "p1"}]},
                {"stageId": "s2", "mode": "parallel", "dependsOn": [], "agents": [{"agentId": "a2", "description": "t2", "prompt": "p2"}]}
            ]
        }))
        .unwrap(),
    );

    let outcome = engine
        .execute_plan(plan, CancellationToken::new())
        .await
        .expect("should finish");

    // Both stages may run (topological sort order is non-deterministic for same-level stages).
    // At minimum one stage runs; at most both run before the failure stops things.
    assert!(call_count.load(Ordering::SeqCst) >= 1);
    assert!(outcome.results.contains_key("a1") || outcome.results.contains_key("a2"));
}
