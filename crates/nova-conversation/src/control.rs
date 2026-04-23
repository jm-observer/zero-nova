/// Represents the stable control state attached to a Session.
pub struct ControlState {
    pub active_agent: String,
}

impl ControlState {
    pub fn new(default_agent: &str) -> Self {
        Self {
            active_agent: default_agent.to_string(),
        }
    }
}
