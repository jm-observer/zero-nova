use crate::provider::types::StreamEvent;
use anyhow::{anyhow, Result};
use serde_json::from_str;

pub struct SseParser {
    // buffer for incoming SSE data
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Add new data to the buffer.
    pub fn feed(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Try to parse the next event from the buffer.
    pub fn next_event(&mut self) -> Result<Option<StreamEvent>> {
        // Split on double newline which terminates an SSE message
        if let Some(pos) = self.find_double_newline() {
            let raw_bytes = self.buffer[..pos].to_vec();
            // Consume up to pos + 2 (the \n\n)
            let _ = self.buffer.drain(..pos + 2);

            let raw_str = std::str::from_utf8(&raw_bytes).map_err(|e| anyhow!("Invalid UTF-8 in SSE frame: {}", e))?;
            let raw = raw_str.trim();

            // Remove "data: " prefix if present
            let json_str = if let Some(rest) = raw.strip_prefix("data: ") {
                rest.trim()
            } else {
                raw
            };

            if json_str.is_empty() || json_str == "[DONE]" {
                // If it was just an empty line or [DONE], try getting the next one recursively
                return self.next_event();
            }

            // Parse JSON into StreamEvent
            let event: StreamEvent =
                from_str(json_str).map_err(|e| anyhow!("Failed to parse SSE JSON: {}, content: {}", e, json_str))?;
            return Ok(Some(event));
        }
        Ok(None)
    }

    fn find_double_newline(&self) -> Option<usize> {
        (0..self.buffer.len().saturating_sub(1)).find(|&i| self.buffer[i] == b'\n' && self.buffer[i + 1] == b'\n')
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}
