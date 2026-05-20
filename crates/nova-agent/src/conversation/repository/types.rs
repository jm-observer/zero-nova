#[derive(Debug, Clone)]
pub struct SessionUsageAggregate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct UsageQualityCounts {
    pub total_turns: u32,
    pub turns_with_unknown_cache: u32,
    pub turns_with_missing_usage: u32,
}

pub type SessionRow = (
    String,
    String,
    String,
    i64,
    i64,
    crate::conversation::control::ControlState,
    Option<String>, // parent_session_id
    Option<String>, // parent_tool_use_id
);
