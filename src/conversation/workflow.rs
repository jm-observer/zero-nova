use super::control::PendingInteraction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCandidate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStage {
    /// 收集用户需求与约束
    GatherRequirements,
    /// 搜索候选方案
    Discover,
    /// 展示对比，等待用户选择
    AwaitSelection,
    /// 用户已选择，等待部署确认
    AwaitExecutionConfirm,
    /// 部署执行中
    Executing,
    /// 执行完成，等待测试输入
    AwaitTestInput,
    /// 测试中
    Testing,
    /// 流程完成
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowType {
    Solution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub workflow_type: WorkflowType,
    pub topic: Option<String>, // "TTS" / "向量数据库" / "图片生成"
    pub stage: WorkflowStage,
    pub constraints: serde_json::Value,     // 用户提供的约束条件
    pub candidates: Vec<WorkflowCandidate>, // 搜索到的候选方案
    pub selected_candidate: Option<String>, // 用户选择的方案 id
    pub created_at: i64,
}

impl WorkflowState {
    pub fn new(topic: String) -> Self {
        Self {
            workflow_type: WorkflowType::Solution,
            topic: Some(topic),
            stage: WorkflowStage::GatherRequirements,
            constraints: serde_json::json!({}),
            candidates: Vec::new(),
            selected_candidate: None,
            created_at: chrono::Utc::now().timestamp(),
        }
    }
}

pub struct WorkflowAdvanceResult {
    pub new_pending: Option<PendingInteraction>,
    pub messages: Vec<String>,
    pub stage_changed: bool,
}

pub struct WorkflowEngine;

impl WorkflowEngine {
    /// 判断用户输入是否像在继续 workflow
    pub fn looks_like_continuation(workflow: &WorkflowState) -> bool {
        matches!(
            workflow.stage,
            WorkflowStage::GatherRequirements
                | WorkflowStage::AwaitSelection
                | WorkflowStage::AwaitExecutionConfirm
                | WorkflowStage::AwaitTestInput
        )
    }

    /// 执行 workflow 的当前阶段
    pub async fn advance<C: crate::provider::LlmClient>(
        workflow: &mut WorkflowState,
        input: &str,
        _agent: &crate::agent::AgentRuntime<C>,
        _event_tx: tokio::sync::mpsc::Sender<crate::event::AgentEvent>,
    ) -> anyhow::Result<WorkflowAdvanceResult> {
        let mut messages = Vec::new();
        let mut stage_changed = false;

        match workflow.stage {
            WorkflowStage::GatherRequirements => {
                workflow.constraints = serde_json::json!({ "user_input": input });
                workflow.stage = WorkflowStage::Discover;
                messages.push("正在为您搜索合适的方案...".to_string());

                workflow.candidates = vec![
                    WorkflowCandidate {
                        id: "c1".to_string(),
                        name: "方案 A".to_string(),
                        description: "高性能方案".to_string(),
                        pros: vec!["快".to_string()],
                        cons: vec!["贵".to_string()],
                    },
                    WorkflowCandidate {
                        id: "c2".to_string(),
                        name: "方案 B".to_string(),
                        description: "高性价比方案".to_string(),
                        pros: vec!["省钱".to_string()],
                        cons: vec!["稍慢".to_string()],
                    },
                ];
                workflow.stage = WorkflowStage::AwaitSelection;
                stage_changed = true;
                messages.push("已找到以下方案，请选择：".to_string());
                messages.push("1. 方案 A (高性能)\n2. 方案 B (高性价比)".to_string());
            }
            WorkflowStage::AwaitSelection => {
                if input.contains('1') {
                    workflow.selected_candidate = Some("c1".to_string());
                } else if input.contains('2') {
                    workflow.selected_candidate = Some("c2".to_string());
                } else {
                    messages.push("请明确选择 1 或 2".to_string());
                    return Ok(WorkflowAdvanceResult {
                        new_pending: None,
                        messages,
                        stage_changed: false,
                    });
                }

                workflow.stage = WorkflowStage::AwaitExecutionConfirm;
                stage_changed = true;
                messages.push("已为您选定方案。是否立即开始部署执行？(确认/取消)".to_string());
            }
            WorkflowStage::AwaitExecutionConfirm => {
                if input.contains("确") || input.contains("是") || input.contains("ok") {
                    workflow.stage = WorkflowStage::Executing;
                    stage_changed = true;
                    messages.push("正在开始部署任务...".to_string());
                } else {
                    workflow.stage = WorkflowStage::GatherRequirements;
                    stage_changed = true;
                    messages.push("已取消任务。".to_string());
                }
            }
            _ => {
                messages.push("当前阶段暂不支持通过此方式推进。".to_string());
            }
        }

        Ok(WorkflowAdvanceResult {
            new_pending: None,
            messages,
            stage_changed,
        })
    }
}
