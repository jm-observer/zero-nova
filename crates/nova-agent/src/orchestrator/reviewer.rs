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
    use super::build_review_prompt;
    use crate::orchestrator::scheduler::{SubAgentResult, SubAgentStatus};
    use std::collections::HashMap;

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
}
