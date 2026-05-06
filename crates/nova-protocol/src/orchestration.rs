use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 编排计划发布事件（Orchestrator 完成拆分后广播）
/// 通过 ProgressEvent { kind="orchestration_plan", args=<this> } 携带
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationPlanEvent {
    pub session_id: String,
    pub plan_id: String,
    pub description: String,
    pub stages: Vec<StageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageSummary {
    pub stage_id: String,
    /// "parallel" | "serial"
    pub mode: String,
    pub depends_on: Vec<String>,
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    /// 编排 Plan 内唯一，如 "agent-1"
    pub agent_id: String,
    pub description: String,
    pub subagent_type: String,
}

/// 子 Agent 执行结果摘要
/// 通过 ProgressEvent { kind="sub_agent_complete", args=<this> } 携带
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentCompleteArgs {
    pub agent_id: String,
    pub stage_id: String,
    /// "success" | "failed" | "cancelled"
    pub status: String,
    pub output_summary: String,
}

/// Stage 完成事件
/// 通过 ProgressEvent { kind="stage_complete", args=<this> } 携带
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageCompleteArgs {
    pub stage_id: String,
    /// "parallel" | "serial"
    pub mode: String,
    pub all_success: bool,
}

/// 编排完成事件
/// 通过 ProgressEvent { kind="orchestration_complete", args=<this> } 携带
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationCompleteArgs {
    pub plan_id: String,
    pub overall_success: bool,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestration_plan_event_roundtrip() {
        let event = OrchestrationPlanEvent {
            session_id: "sess-1".to_string(),
            plan_id: "plan-1".to_string(),
            description: "编排测试".to_string(),
            stages: vec![StageSummary {
                stage_id: "s1".to_string(),
                mode: "parallel".to_string(),
                depends_on: vec![],
                agents: vec![AgentSummary {
                    agent_id: "agent-1".to_string(),
                    description: "任务A".to_string(),
                    subagent_type: "general-purpose".to_string(),
                }],
            }],
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["planId"], "plan-1");
        assert_eq!(json["stages"][0]["stageId"], "s1");
        assert_eq!(json["stages"][0]["agents"][0]["agentId"], "agent-1");

        let restored: OrchestrationPlanEvent = serde_json::from_value(json).unwrap();
        assert_eq!(restored.plan_id, "plan-1");
        assert_eq!(restored.stages[0].agents[0].agent_id, "agent-1");
    }

    #[test]
    fn test_sub_agent_complete_args_roundtrip() {
        let args = SubAgentCompleteArgs {
            agent_id: "agent-1".to_string(),
            stage_id: "s1".to_string(),
            status: "success".to_string(),
            output_summary: "完成文件分析".to_string(),
        };

        let json = serde_json::to_value(&args).unwrap();
        assert_eq!(json["agentId"], "agent-1");
        assert_eq!(json["status"], "success");

        let restored: SubAgentCompleteArgs = serde_json::from_value(json).unwrap();
        assert_eq!(restored.status, "success");
    }

    #[test]
    fn test_stage_complete_args_roundtrip() {
        let args = StageCompleteArgs {
            stage_id: "s1".to_string(),
            mode: "parallel".to_string(),
            all_success: true,
        };

        let json = serde_json::to_value(&args).unwrap();
        assert_eq!(json["stageId"], "s1");
        assert_eq!(json["allSuccess"], true);

        let restored: StageCompleteArgs = serde_json::from_value(json).unwrap();
        assert!(restored.all_success);
    }

    #[test]
    fn test_orchestration_complete_args_roundtrip() {
        let args = OrchestrationCompleteArgs {
            plan_id: "plan-1".to_string(),
            overall_success: true,
            summary: "所有子任务成功完成".to_string(),
        };

        let json = serde_json::to_value(&args).unwrap();
        assert_eq!(json["planId"], "plan-1");
        assert_eq!(json["overallSuccess"], true);

        let restored: OrchestrationCompleteArgs = serde_json::from_value(json).unwrap();
        assert_eq!(restored.plan_id, "plan-1");
    }
}
