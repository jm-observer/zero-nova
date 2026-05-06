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
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub context_files: Vec<String>,
    #[serde(default)]
    pub output_format: Option<String>,
}

pub fn parse_and_validate(plan_json: &str) -> Result<OrchestrationPlan> {
    let mut plan: OrchestrationPlan = serde_json::from_str(plan_json)?;
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
    use super::parse_and_validate;

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
}
