use crate::provider::types::StreamEvent;
use anyhow::{Result, anyhow};
use serde_json::from_str;

/// Represents a raw SSE event before JSON parsing.
#[derive(Debug)]
pub enum RawSseEvent {
    /// Contains the raw JSON string from the 'data:' field.
    Data(String),
    /// Indicates the stream has finished (e.g., '[DONE]').
    Done,
}

/// Parser for Server Sent Events (SSE) streams.
pub struct SseParser {
    // buffer for incoming SSE data
    buffer: Vec<u8>,
}

impl SseParser {
    /// Creates a new `SseParser` with an empty buffer.
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Add new data to the buffer.
    /// Feeds a chunk of data into the parser's buffer.
    pub fn feed(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Extract the next raw SSE frame content without JSON deserialization.
    /// Returns Ok(None) if no complete frame is available in the buffer.
    /// Returns Ok(Some(RawSseEvent::Done)) when "[DONE]" is encountered.
    /// Returns Ok(Some(RawSseEvent::Data(String))) with the raw JSON string.
    pub fn next_raw(&mut self) -> Result<Option<RawSseEvent>> {
        if let Some(pos) = self.find_double_newline() {
            let raw_bytes = self.buffer[..pos].to_vec();

            // Determine terminator length to drain buffer correctly
            let terminator_len =
                if pos + 1 < self.buffer.len() && self.buffer[pos] == b'\n' && self.buffer[pos + 1] == b'\n' {
                    2
                } else if pos + 3 < self.buffer.len()
                    && self.buffer[pos] == b'\r'
                    && self.buffer[pos + 1] == b'\n'
                    && self.buffer[pos + 2] == b'\r'
                    && self.buffer[pos + 3] == b'\n'
                {
                    4
                } else {
                    2
                };

            let _ = self.buffer.drain(..pos + terminator_len);
            let raw_str = std::str::from_utf8(&raw_bytes).map_err(|e| anyhow!("Invalid UTF-8 in SSE frame: {}", e))?;

            let mut data_content = String::new();
            for line in raw_str.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("data: ") {
                    data_content.push_str(rest);
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_content.push_str(rest.trim_start());
                }
            }

            let json_str = data_content.trim();
            if json_str.is_empty() {
                return self.next_raw();
            }
            if json_str == "[DONE]" {
                return Ok(Some(RawSseEvent::Done));
            }
            return Ok(Some(RawSseEvent::Data(json_str.to_string())));
        }
        Ok(None)
    }

    /// Try to parse the next event from the buffer.
    /// Attempts to parse the next `StreamEvent` from the internal buffer.
    pub fn next_event(&mut self) -> Result<Option<StreamEvent>> {
        match self.next_raw()? {
            Some(RawSseEvent::Done) => {
                log::info!("SSE stream received [DONE]");
                Ok(None)
            }
            Some(RawSseEvent::Data(json_str)) => {
                let event: StreamEvent = from_str(&json_str).map_err(|e| {
                    log::error!("Failed to parse SSE JSON: {}, content: {}", e, json_str);
                    anyhow!("Failed to parse SSE JSON: {}, content: {}", e, json_str)
                })?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    /// Finds the position of a double newline terminator in the buffer.
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
                return Some(i); // Return index of the first \r
            }
        }
        None
    }
}

/// Provides a default constructor for `SseParser`.
impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}
