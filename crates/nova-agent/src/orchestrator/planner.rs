use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationPlan {
    pub plan_id: String,
    pub description: String,
    pub stages: Vec<ExecutionStage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStage {
    pub stage_id: String,
    pub mode: StageMode,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub agents: Vec<AgentRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageMode {
    Parallel,
    Serial,
}

impl StageMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Parallel => "parallel",
            Self::Serial => "serial",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    pub agent_id: String,
    /// 已废弃：由模型直接填写的 subagent_type（Plan 1 标记为 deprecated）。
    /// 运行时优先使用 catalog 中的 agent 选择。
    #[serde(default = "default_subagent_type")]
    #[serde(alias = "agentType")]
    pub subagent_type: String,
    /// 受控 catalog 中的 agent 选择（Plan 1 新增）。
    /// 当存在时优先使用，否则回退到 subagent_type。
    #[serde(default)]
    pub agent_selection: Option<String>,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub context_files: Vec<String>,
    #[serde(default)]
    pub output_format: Option<String>,
}

fn default_subagent_type() -> String {
    "nova".to_string()
}

pub fn parse_and_validate(plan_json: &str) -> Result<OrchestrationPlan> {
    let plan: OrchestrationPlan = serde_json::from_str(plan_json)?;
    validate_and_sort(plan)
}

pub fn validate_and_sort(mut plan: OrchestrationPlan) -> Result<OrchestrationPlan> {
    if plan.plan_id.trim().is_empty() {
        bail!("plan_id cannot be empty");
    }

    let stage_ids: HashSet<String> = plan.stages.iter().map(|s| s.stage_id.clone()).collect();
    if stage_ids.len() != plan.stages.len() {
        bail!("duplicate stage_id found in plan");
    }

    for stage in &plan.stages {
        for dep in &stage.depends_on {
            if !stage_ids.contains(dep) {
                bail!("stage '{}' depends on unknown stage '{}'", stage.stage_id, dep);
            }
        }
    }

    validate_unique_agent_ids(&plan)?;
    plan.stages = topological_sort(&plan.stages)?;
    Ok(plan)
}

fn validate_unique_agent_ids(plan: &OrchestrationPlan) -> Result<()> {
    let mut seen = HashSet::new();
    for stage in &plan.stages {
        for agent in &stage.agents {
            if !seen.insert(agent.agent_id.clone()) {
                bail!("duplicate agent_id: {}", agent.agent_id);
            }
        }
    }
    Ok(())
}

fn topological_sort(stages: &[ExecutionStage]) -> Result<Vec<ExecutionStage>> {
    let mut indegree = HashMap::<String, usize>::new();
    let mut graph = HashMap::<String, Vec<String>>::new();
    let stage_map: HashMap<String, ExecutionStage> = stages
        .iter()
        .cloned()
        .map(|stage| (stage.stage_id.clone(), stage))
        .collect();

    for stage in stages {
        indegree.entry(stage.stage_id.clone()).or_insert(0);
        for dep in &stage.depends_on {
            graph.entry(dep.clone()).or_default().push(stage.stage_id.clone());
            *indegree.entry(stage.stage_id.clone()).or_insert(0) += 1;
        }
    }

    let mut queue = VecDeque::new();
    for (stage_id, degree) in &indegree {
        if *degree == 0 {
            queue.push_back(stage_id.clone());
        }
    }

    let mut ordered = Vec::new();
    while let Some(stage_id) = queue.pop_front() {
        if let Some(stage) = stage_map.get(&stage_id) {
            ordered.push(stage.clone());
        }
        if let Some(next_stages) = graph.get(&stage_id) {
            for next in next_stages {
                if let Some(deg) = indegree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != stages.len() {
        bail!("cyclic dependency detected in orchestration stages");
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::{parse_and_validate, StageMode};
    use serde_json::json;

    fn plan_json_chain(stage_count: usize) -> String {
        let stages: Vec<_> = (1..=stage_count)
            .map(|index| {
                let stage_id = format!("s{index}");
                let depends_on = if index == 1 {
                    Vec::new()
                } else {
                    vec![format!("s{}", index - 1)]
                };
                json!({
                    "stageId": stage_id,
                    "mode": "parallel",
                    "dependsOn": depends_on,
                    "agents": [{
                        "agentId": format!("a{index}"),
                        "description": format!("task-{index}"),
                        "prompt": "do it"
                    }]
                })
            })
            .collect();
        serde_json::to_string(&json!({
            "planId": "p1",
            "description": "chain",
            "stages": stages
        }))
        .expect("plan should serialize")
    }

    fn plan_json_diamond() -> String {
        serde_json::to_string(&json!({
            "planId": "p1",
            "description": "diamond",
            "stages": [
                {
                    "stageId": "s1",
                    "mode": "parallel",
                    "dependsOn": [],
                    "agents": [{"agentId": "a1", "description": "root", "prompt": "do it"}]
                },
                {
                    "stageId": "s2",
                    "mode": "parallel",
                    "dependsOn": ["s1"],
                    "agents": [{"agentId": "a2", "description": "left", "prompt": "do it"}]
                },
                {
                    "stageId": "s3",
                    "mode": "serial",
                    "dependsOn": ["s1"],
                    "agents": [{"agentId": "a3", "description": "right", "prompt": "do it"}]
                },
                {
                    "stageId": "s4",
                    "mode": "serial",
                    "dependsOn": ["s2", "s3"],
                    "agents": [{"agentId": "a4", "description": "merge", "prompt": "do it"}]
                }
            ]
        }))
        .expect("plan should serialize")
    }

    #[test]
    fn rejects_unknown_dependency() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[{"stageId":"s1","mode":"parallel","dependsOn":["s99"],"agents":[]}]
        }"#;
        assert!(parse_and_validate(json).is_err());
    }

    #[test]
    fn rejects_cycle() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[
            {"stageId":"s1","mode":"parallel","dependsOn":["s2"],"agents":[]},
            {"stageId":"s2","mode":"serial","dependsOn":["s1"],"agents":[]}
          ]
        }"#;
        assert!(parse_and_validate(json).is_err());
    }

    #[test]
    fn defaults_missing_subagent_type_to_nova() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[
            {"stageId":"s1","mode":"parallel","dependsOn":[],"agents":[{"agentId":"a1","description":"task","prompt":"do it"}]}
          ]
        }"#;
        let plan = parse_and_validate(json).expect("plan should parse");
        assert_eq!(plan.stages[0].agents[0].subagent_type, "nova");
    }

    #[test]
    fn valid_multi_stage_topological_sort() {
        let plan = parse_and_validate(&plan_json_chain(3)).expect("plan should parse");
        let stage_ids: Vec<_> = plan.stages.into_iter().map(|stage| stage.stage_id).collect();
        assert_eq!(stage_ids, vec!["s1", "s2", "s3"]);
    }

    #[test]
    fn duplicate_stage_ids_rejected() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[
            {"stageId":"s1","mode":"parallel","dependsOn":[],"agents":[]},
            {"stageId":"s1","mode":"serial","dependsOn":[],"agents":[]}
          ]
        }"#;
        let error = parse_and_validate(json).expect_err("plan should fail");
        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn duplicate_agent_ids_rejected() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[
            {"stageId":"s1","mode":"parallel","dependsOn":[],"agents":[{"agentId":"a1","description":"one","prompt":"do"}]},
            {"stageId":"s2","mode":"serial","dependsOn":[],"agents":[{"agentId":"a1","description":"two","prompt":"do"}]}
          ]
        }"#;
        let error = parse_and_validate(json).expect_err("plan should fail");
        assert!(error.to_string().contains("duplicate agent_id"));
    }

    #[test]
    fn empty_plan_id_rejected() {
        let json = r#"{
          "planId":"   ",
          "description":"d",
          "stages":[]
        }"#;
        assert!(parse_and_validate(json).is_err());
    }

    #[test]
    fn diamond_dependency_sorts_correctly() {
        let plan = parse_and_validate(&plan_json_diamond()).expect("plan should parse");
        let positions: std::collections::HashMap<_, _> = plan
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| (stage.stage_id.as_str(), index))
            .collect();
        assert!(positions["s1"] < positions["s2"]);
        assert!(positions["s1"] < positions["s3"]);
        assert!(positions["s2"] < positions["s4"]);
        assert!(positions["s3"] < positions["s4"]);
    }

    #[test]
    fn invalid_json_rejected() {
        assert!(parse_and_validate("{{not json}}").is_err());
    }

    #[test]
    fn agent_selection_field_deserialized() {
        let json = r#"{
          "planId":"p1",
          "description":"d",
          "stages":[
            {"stageId":"s1","mode":"parallel","dependsOn":[],"agents":[{"agentId":"a1","agentSelection":"developer","description":"task","prompt":"do it"}]}
          ]
        }"#;
        let plan = parse_and_validate(json).expect("plan should parse");
        assert_eq!(plan.stages[0].agents[0].agent_selection.as_deref(), Some("developer"));
    }

    #[test]
    fn stage_mode_as_str() {
        assert_eq!(StageMode::Parallel.as_str(), "parallel");
        assert_eq!(StageMode::Serial.as_str(), "serial");
    }
}
