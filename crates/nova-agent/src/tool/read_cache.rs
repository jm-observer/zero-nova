use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRange {
    pub offset_start: usize,
    pub offset_end: usize,
    pub returned_line_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FileReadState {
    pub ranges: Vec<ReadRange>,
    pub last_excerpt_fingerprint: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct TurnReadState {
    pub files: HashMap<String, FileReadState>,
}

impl TurnReadState {
    pub fn file_state(&self, canonical_path: &str) -> Option<&FileReadState> {
        self.files.get(canonical_path)
    }

    pub fn record_range(&mut self, canonical_path: String, range: ReadRange, excerpt_fingerprint: u64) {
        let entry = self.files.entry(canonical_path).or_default();
        entry.ranges.push(range);
        entry.last_excerpt_fingerprint = Some(excerpt_fingerprint);
    }
}
