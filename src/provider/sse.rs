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
            // Consume up to pos + 2 (the \n\n or whatever terminator)
            // SSE standard says \r\n\r\n or \n\n
            let terminator_len = if self.buffer.get(pos + 1) == Some(&b'\n') {
                if self.buffer.get(pos) == Some(&b'\r') {
                    // This case is actually handled by find_double_newline mapping to the first \n
                    // Let's refine find_double_newline to be more robust.
                    2
                } else {
                    2
                }
            } else {
                2
            };

            let _ = self.buffer.drain(..pos + terminator_len);
            let raw_str = std::str::from_utf8(&raw_bytes).map_err(|e| anyhow!("Invalid UTF-8 in SSE frame: {}", e))?;

            log::debug!("Parsing SSE frame: {}", raw_str);

            let mut data_content = String::new();
            for line in raw_str.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("data: ") {
                    data_content.push_str(rest);
                } else if line == "data:" {
                    // Empty data line
                }
            }

            let json_str = data_content.trim();

            if json_str.is_empty() {
                return self.next_event();
            }

            if json_str == "[DONE]" {
                log::info!("SSE stream received [DONE]");
                return Ok(None);
            }

            // Parse JSON into StreamEvent
            let event: StreamEvent = from_str(json_str).map_err(|e| {
                log::error!("Failed to parse SSE JSON: {}, content: {}", e, json_str);
                anyhow!("Failed to parse SSE JSON: {}, content: {}", e, json_str)
            })?;
            return Ok(Some(event));
        }
        Ok(None)
    }

    fn find_double_newline(&self) -> Option<usize> {
        for i in 0..self.buffer.len().saturating_sub(1) {
            if self.buffer[i] == b'\n' && self.buffer[i + 1] == b'\n' {
                return Some(i);
            }
            if i + 3 < self.buffer.len()
                && self.buffer[i] == b'\r'
                && self.buffer[i + 1] == b'\n'
                && self.buffer[i + 2] == b'\r'
                && self.buffer[i + 3] == b'\n'
            {
                return Some(i + 2); // Return index of the second \r
            }
        }
        None
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}
