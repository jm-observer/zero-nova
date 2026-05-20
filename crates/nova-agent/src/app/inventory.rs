use crate::tool::{DeferredToolRepresentation, RegisteredToolDefinition};

#[derive(Clone)]
pub struct ToolInventoryView {
    pub loaded: Vec<RegisteredToolDefinition>,
    pub deferred: Vec<DeferredToolRepresentation>,
}
