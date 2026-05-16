use super::scheduler::{SubAgentResult, SubAgentStatus};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub success: bool,
    pub issues: Vec<String>,
    pub summary: String,
    #[serde(default)]
    pub retry_agents: Vec<String>,
}

pub fn build_review_prompt(plan_description: &str, results: &HashMap<String, SubAgentResult>) -> String {
    let mut summaries = Vec::new();
    for (agent_id, result) in results {
        let status = match result.status {
            SubAgentStatus::Success => "success",
            SubAgentStatus::Failed => "failed",
            SubAgentStatus::Cancelled => "cancelled",
        };
        summaries.push(format!(
            "## Agent: {}\nStage: {}\nStatus: {}\nOutput:\n{}\nError:\n{}",
            agent_id,
            result.stage_id,
            status,
            if result.output.is_empty() {
                "<empty>"
            } else {
                &result.output
            },
            result.error.as_deref().unwrap_or("<none>")
        ));
    }

    format!(
        "你是 Review Agent。目标：{}\n\n{}\n\n请返回 JSON：{{\"success\":true/false,\"issues\":[...],\"summary\":\"...\",\"retryAgents\":[...]}}",
        plan_description,
        summaries.join("\n\n---\n\n")
    )
}

pub fn parse_review_result(raw: &str) -> Result<ReviewResult> {
    serde_json::from_str(raw).context("failed to parse review result JSON")
}

#[cfg(test)]
mod tests {
    use super::{build_review_prompt, parse_review_result};
    use crate::orchestrator::scheduler::{SubAgentResult, SubAgentStatus};
    use std::collections::HashMap;

    fn success_result(agent_id: &str, output: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: "plan-1".to_string(),
            agent_id: agent_id.to_string(),
            stage_id: "s1".to_string(),
            status: SubAgentStatus::Success,
            output: output.to_string(),
            error: None,
        }
    }

    fn failed_result(agent_id: &str, error: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: "plan-1".to_string(),
            agent_id: agent_id.to_string(),
            stage_id: "s1".to_string(),
            status: SubAgentStatus::Failed,
            output: String::new(),
            error: Some(error.to_string()),
        }
    }

    fn cancelled_result(agent_id: &str) -> SubAgentResult {
        SubAgentResult {
            plan_id: "plan-1".to_string(),
            agent_id: agent_id.to_string(),
            stage_id: "s1".to_string(),
            status: SubAgentStatus::Cancelled,
            output: String::new(),
            error: Some("cancelled".to_string()),
        }
    }

    #[test]
    fn review_prompt_includes_failure_details() {
        let mut results = HashMap::new();
        results.insert(
            "a1".to_string(),
            SubAgentResult {
                plan_id: "plan-1".to_string(),
                agent_id: "a1".to_string(),
                stage_id: "s1".to_string(),
                status: SubAgentStatus::Failed,
                output: String::new(),
                error: Some("tool failed".to_string()),
            },
        );

        let prompt = build_review_prompt("repair", &results);
        assert!(prompt.contains("Status: failed"));
        assert!(prompt.contains("Error:\ntool failed"));
    }

    #[test]
    fn review_prompt_success_agents() {
        let mut results = HashMap::new();
        results.insert("a1".to_string(), success_result("a1", "good result"));

        let prompt = build_review_prompt("review success", &results);
        assert!(prompt.contains("Status: success"));
        assert!(prompt.contains("good result"));
    }

    #[test]
    fn review_prompt_cancelled_agents() {
        let mut results = HashMap::new();
        results.insert("a1".to_string(), cancelled_result("a1"));

        let prompt = build_review_prompt("review cancelled", &results);
        assert!(prompt.contains("Status: cancelled"));
    }

    #[test]
    fn review_prompt_multiple_agents() {
        let mut results = HashMap::new();
        results.insert("a1".to_string(), success_result("a1", "done"));
        results.insert("a2".to_string(), failed_result("a2", "bad output"));
        results.insert("a3".to_string(), cancelled_result("a3"));

        let prompt = build_review_prompt("review many", &results);
        assert!(prompt.contains("## Agent: a1"));
        assert!(prompt.contains("## Agent: a2"));
        assert!(prompt.contains("## Agent: a3"));
        assert!(prompt.contains("\n\n---\n\n"));
    }

    #[test]
    fn review_prompt_empty_output() {
        let mut results = HashMap::new();
        results.insert("a1".to_string(), success_result("a1", ""));

        let prompt = build_review_prompt("review empty output", &results);
        assert!(prompt.contains("Output:\n<empty>"));
    }

    #[test]
    fn parse_review_result_valid() {
        let raw = r#"{"success": true, "issues": [], "retryAgents": [], "summary": "All good."}"#;
        let result = parse_review_result(raw).expect("result should parse");

        assert!(result.success);
        assert!(result.issues.is_empty());
        assert!(result.retry_agents.is_empty());
        assert_eq!(result.summary, "All good.");
    }

    #[test]
    fn parse_review_result_invalid() {
        assert!(parse_review_result("not json at all").is_err());
    }

    #[test]
    fn parse_review_result_with_issues() {
        let raw = r#"{
          "success": false,
          "issues": ["missing file", "wrong format"],
          "retryAgents": ["a2"],
          "summary": "Partial failure."
        }"#;
        let result = parse_review_result(raw).expect("result should parse");

        assert!(!result.success);
        assert_eq!(result.issues.len(), 2);
        assert_eq!(result.retry_agents, vec!["a2"]);
        assert_eq!(result.summary, "Partial failure.");
    }

    #[test]
    fn review_prompt_empty_results() {
        let prompt = build_review_prompt("no results", &HashMap::new());
        assert!(prompt.contains("你是 Review Agent。目标：no results"));
        assert!(prompt.contains("请返回 JSON"));
    }
}
