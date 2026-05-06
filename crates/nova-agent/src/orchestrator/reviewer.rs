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
            "## Agent: {}\nStage: {}\nStatus: {}\nOutput:\n{}",
            agent_id, result.stage_id, status, result.output
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
