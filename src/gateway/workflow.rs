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
    pub new_pending: Option<crate::gateway::control::PendingInteraction>,
    pub messages: Vec<String>,
    pub stage_changed: bool,
}

pub struct WorkflowEngine;

impl WorkflowEngine {
    /// 判断用户输入是否像在继续 workflow
    /// 初版规则：如果 workflow 处于 Await* 阶段，任何输入都视为 workflow 回应
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
                // 模拟：用户提供了约束，自动进入 Discover 阶段
                workflow.constraints = serde_json::json!({ "user_input": input });
                workflow.stage = WorkflowStage::Discover;
                messages.push("正在为您搜索合适的方案...".to_string());

                // 模拟发现候选方案
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
                // 模拟：用户输入了数字或名称进行选择
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
                    workflow.stage = WorkflowStage::GatherRequirements; // 简单处理：取消则回到起点
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_initial_stage() {
        let wf = WorkflowState::new("TTS".to_string());
        assert_eq!(wf.stage, WorkflowStage::GatherRequirements);
    }

    #[test]
    fn test_looks_like_continuation() {
        let mut wf = WorkflowState::new("Test".to_string());

        // GatherRequirements 阶段
        assert!(WorkflowEngine::looks_like_continuation(&wf));

        // 切换到 Discover
        wf.stage = WorkflowStage::Discover;
        assert!(!WorkflowEngine::looks_like_continuation(&wf));

        // 切换到 AwaitSelection
        wf.stage = WorkflowStage::AwaitSelection;
        assert!(WorkflowEngine::looks_like_continuation(&wf));
    }

    #[tokio::test]
    async fn test_workflow_advance_flow() {
        use crate::agent::{AgentConfig, AgentRuntime};
        use crate::provider::{LlmClient, ModelConfig, StreamReceiver};
        use crate::tool::ToolRegistry;
        use async_trait::async_trait;
        use std::time::Duration;

        struct DummyClient;
        #[async_trait]
        impl LlmClient for DummyClient {
            async fn stream(
                &self,
                _msgs: &[crate::message::Message],
                _tools: &[crate::provider::types::ToolDefinition],
                _conf: &ModelConfig,
            ) -> anyhow::Result<Box<dyn StreamReceiver>> {
                unimplemented!()
            }
        }

        let client = DummyClient;
        let tools = ToolRegistry::new();
        let agent_config = AgentConfig {
            max_iterations: 1,
            model_config: ModelConfig {
                model: "test".to_string(),
                max_tokens: 10,
                temperature: None,
                top_p: None,
                thinking_budget: None,
                reasoning_effort: None,
            },
            tool_timeout: Duration::from_secs(1),
        };
        let agent = AgentRuntime::new(client, tools, agent_config);
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let mut wf = WorkflowState::new("TTS".to_string());

        // 1. GatherRequirements -> AwaitSelection (模拟 Discover 过程)
        let res = WorkflowEngine::advance(&mut wf, "我需要一个快速的 TTS", &agent, tx.clone())
            .await
            .unwrap();
        assert!(res.stage_changed);
        assert_eq!(wf.stage, WorkflowStage::AwaitSelection);
        assert!(res.messages[0].contains("搜索"));

        // 2. AwaitSelection -> AwaitExecutionConfirm
        let res = WorkflowEngine::advance(&mut wf, "选 1", &agent, tx.clone())
            .await
            .unwrap();
        assert!(res.stage_changed);
        assert_eq!(wf.stage, WorkflowStage::AwaitExecutionConfirm);
        assert_eq!(wf.selected_candidate, Some("c1".to_string()));

        // 3. AwaitExecutionConfirm -> Executing
        let res = WorkflowEngine::advance(&mut wf, "确认", &agent, tx).await.unwrap();
        assert!(res.stage_changed);
        assert_eq!(wf.stage, WorkflowStage::Executing);
    }
}
