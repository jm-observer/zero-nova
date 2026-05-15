use crate::agent_catalog::AgentRegistry;

pub trait AgentRegistrySnapshot: Send + Sync {
    fn current(&self) -> AgentRegistry;
}
