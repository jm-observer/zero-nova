use crate::message::ContentBlock;

pub(super) fn has_loop_guard_rejection(blocks: &[ContentBlock]) -> bool {
    blocks.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolResult {
                output,
                is_error: true,
                ..
            } if output.starts_with("System Guard:")
        )
    })
}
