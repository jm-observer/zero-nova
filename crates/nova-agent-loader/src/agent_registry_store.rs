use arc_swap::ArcSwap;
use nova_agent::agent_catalog::AgentRegistry;
use std::sync::Arc;

#[derive(Clone)]
pub struct AgentRegistryStore {
    registry: Arc<ArcSwap<AgentRegistry>>,
}

impl AgentRegistryStore {
    pub fn new(initial: AgentRegistry) -> Self {
        Self {
            registry: Arc::new(ArcSwap::from_pointee(initial)),
        }
    }

    pub fn replace(&self, next: AgentRegistry) {
        self.registry.store(Arc::new(next));
    }

    pub fn current(&self) -> AgentRegistry {
        self.registry.load_full().as_ref().clone()
    }
}
