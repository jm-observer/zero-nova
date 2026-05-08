use crate::message::ContentBlock;
use serde_json::{Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const DUPLICATE_WARNING_THRESHOLD: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DuplicateReadMode {
    WarnThenReject,
    WarnOnly,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoopGuardConfig {
    pub enabled: bool,
    pub max_consecutive_duplicate_tool_calls: usize,
    pub max_stalled_iterations: usize,
    pub duplicate_read_mode: DuplicateReadMode,
    pub iteration_trim_ratio: f32,
}

impl Default for LoopGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_consecutive_duplicate_tool_calls: 2,
            max_stalled_iterations: 3,
            duplicate_read_mode: DuplicateReadMode::WarnThenReject,
            iteration_trim_ratio: 0.8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallSignature {
    pub tool_name: String,
    pub canonical_primary_target: Option<String>,
    pub normalized_input: String,
    pub input_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssistantFingerprint {
    pub prefix_hash: u64,
    pub suffix_hash: u64,
    pub total_len: usize,
}

#[derive(Debug, Clone)]
pub enum LoopGuardDecision {
    Allow,
    AllowWithWarning { message: String },
    Reject { message: String, reason_code: String },
}

#[derive(Debug, Default)]
pub struct LoopGuardState {
    config: LoopGuardConfig,
    last_call_signature: Option<ToolCallSignature>,
    consecutive_duplicate_count: usize,
    last_iteration_fingerprint: Option<(AssistantFingerprint, u64)>,
    stalled_iteration_count: usize,
}

impl LoopGuardState {
    pub fn new(config: LoopGuardConfig) -> Self {
        Self {
            config,
            last_call_signature: None,
            consecutive_duplicate_count: 0,
            last_iteration_fingerprint: None,
            stalled_iteration_count: 0,
        }
    }

    pub fn evaluate_tool_call(&mut self, signature: ToolCallSignature) -> LoopGuardDecision {
        if !self.config.enabled {
            return LoopGuardDecision::Allow;
        }

        if self.last_call_signature.as_ref() == Some(&signature) {
            self.consecutive_duplicate_count += 1;
        } else {
            self.last_call_signature = Some(signature.clone());
            self.consecutive_duplicate_count = 0;
        }

        if self.consecutive_duplicate_count >= self.config.max_consecutive_duplicate_tool_calls
            && self.config.duplicate_read_mode == DuplicateReadMode::WarnThenReject
        {
            return LoopGuardDecision::Reject {
                message: "System Guard: You have repeated the exact same tool call multiple times. Stop retrying this exact request and either continue your analysis from existing results, or change the parameters (for Read, increase `offset` to inspect a new range).".to_string(),
                reason_code: "duplicate_tool_call_rejected".to_string(),
            };
        }

        if self.consecutive_duplicate_count >= DUPLICATE_WARNING_THRESHOLD {
            return LoopGuardDecision::AllowWithWarning {
                message: "System Guard Warning: This tool call is identical to your previous one. Reuse earlier output when possible; for Read, prefer a higher `offset` instead of re-reading the same range.".to_string(),
            };
        }

        LoopGuardDecision::Allow
    }

    pub fn detect_stalled_iteration(
        &mut self,
        assistant_fingerprint: AssistantFingerprint,
        tool_calls_hash: u64,
    ) -> bool {
        if !self.config.enabled {
            return false;
        }
        let current = (assistant_fingerprint, tool_calls_hash);
        if self.last_iteration_fingerprint.as_ref() == Some(&current) {
            self.stalled_iteration_count += 1;
        } else {
            self.last_iteration_fingerprint = Some(current);
            self.stalled_iteration_count = 0;
        }
        self.stalled_iteration_count >= self.config.max_stalled_iterations
    }

    pub fn duplicate_count(&self) -> usize {
        self.consecutive_duplicate_count
    }

    pub fn stalled_count(&self) -> usize {
        self.stalled_iteration_count
    }
}

pub fn build_tool_call_signature(tool_name: &str, input: &Value) -> ToolCallSignature {
    let canonical_tool_name = tool_name.trim().to_string();
    let mut canonical = Map::new();
    canonical.insert("tool".to_string(), Value::String(canonical_tool_name.clone()));

    let target = match canonical_tool_name.as_str() {
        "Read" => {
            let path_val = input.get("file_path").or_else(|| input.get("path"));
            if let Some(p) = path_val {
                canonical.insert("file_path".to_string(), p.clone());
            }
            copy_if_present(input, &mut canonical, "offset");
            copy_if_present(input, &mut canonical, "limit");
            path_val.and_then(Value::as_str).map(|s| s.trim().to_string())
        }
        "Write" | "Edit" => {
            let path_val = input.get("file_path").or_else(|| input.get("path"));
            if let Some(p) = path_val {
                canonical.insert("file_path".to_string(), p.clone());
            }
            path_val.and_then(Value::as_str).map(|s| s.trim().to_string())
        }
        "Bash" => {
            let command = input
                .get("command")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            canonical.insert("command".to_string(), Value::String(command.clone()));
            None
        }
        "Agent" => {
            copy_if_present(input, &mut canonical, "subagent_type");
            if let Some(prompt) = input.get("prompt").and_then(Value::as_str) {
                canonical.insert("prompt_hash".to_string(), Value::String(hash_text(prompt).to_string()));
            }
            input
                .get("subagent_type")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
        }
        _ => {
            canonical.insert("input".to_string(), normalize_json(input));
            None
        }
    };

    let normalized_input = normalize_json(&Value::Object(canonical)).to_string();
    let input_hash = hash_text(&normalized_input);

    ToolCallSignature {
        tool_name: canonical_tool_name,
        canonical_primary_target: target,
        normalized_input,
        input_hash,
    }
}

pub fn assistant_fingerprint_from_blocks(blocks: &[ContentBlock]) -> AssistantFingerprint {
    let text: String = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assistant_fingerprint_from_text(&text)
}

pub fn assistant_fingerprint_from_text(text: &str) -> AssistantFingerprint {
    let prefix: String = text.chars().take(64).collect();
    let chars = text.chars().collect::<Vec<_>>();
    let suffix = if chars.len() <= 64 {
        chars.iter().collect::<String>()
    } else {
        chars[chars.len() - 64..].iter().collect::<String>()
    };
    AssistantFingerprint {
        prefix_hash: hash_text(&prefix),
        suffix_hash: hash_text(&suffix),
        total_len: chars.len(),
    }
}

pub fn tool_calls_hash(signatures: &[ToolCallSignature]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for signature in signatures {
        signature.hash(&mut hasher);
    }
    hasher.finish()
}

fn copy_if_present(input: &Value, dst: &mut Map<String, Value>, key: &str) {
    if let Some(value) = input.get(key) {
        dst.insert(key.to_string(), value.clone());
    }
}

fn normalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let mut normalized = Map::new();
            for key in keys {
                if let Some(inner) = map.get(&key) {
                    normalized.insert(key, normalize_json(inner));
                }
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_json).collect()),
        _ => value.clone(),
    }
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{
        assistant_fingerprint_from_text, build_tool_call_signature, DuplicateReadMode, LoopGuardConfig,
        LoopGuardDecision, LoopGuardState,
    };
    use serde_json::json;

    #[test]
    fn duplicate_call_goes_warning_then_reject() {
        let mut state = LoopGuardState::new(LoopGuardConfig::default());
        let sig = build_tool_call_signature("Read", &json!({"file_path":"a.rs","offset":1,"limit":100}));

        let first = state.evaluate_tool_call(sig.clone());
        let second = state.evaluate_tool_call(sig.clone());
        let third = state.evaluate_tool_call(sig);

        assert!(matches!(first, LoopGuardDecision::Allow));
        assert!(matches!(second, LoopGuardDecision::AllowWithWarning { .. }));
        assert!(matches!(third, LoopGuardDecision::Reject { .. }));
    }

    #[test]
    fn different_read_offset_is_not_duplicate() {
        let mut state = LoopGuardState::new(LoopGuardConfig::default());
        let first = build_tool_call_signature("Read", &json!({"file_path":"a.rs","offset":1,"limit":100}));
        let second = build_tool_call_signature("Read", &json!({"file_path":"a.rs","offset":101,"limit":100}));

        let _ = state.evaluate_tool_call(first);
        let decision = state.evaluate_tool_call(second);
        assert!(matches!(decision, LoopGuardDecision::Allow));
    }

    #[test]
    fn repeated_iteration_triggers_stall() {
        let mut state = LoopGuardState::new(LoopGuardConfig::default());
        let fp = assistant_fingerprint_from_text("same");

        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(state.detect_stalled_iteration(fp, 1));
    }

    #[test]
    fn warn_only_mode_never_rejects_duplicate_call() {
        let mut state = LoopGuardState::new(LoopGuardConfig {
            duplicate_read_mode: DuplicateReadMode::WarnOnly,
            ..LoopGuardConfig::default()
        });
        let sig = build_tool_call_signature("Read", &json!({"file_path":"a.rs","offset":1,"limit":100}));

        let _ = state.evaluate_tool_call(sig.clone());
        let second = state.evaluate_tool_call(sig.clone());
        let third = state.evaluate_tool_call(sig);
        assert!(matches!(second, LoopGuardDecision::AllowWithWarning { .. }));
        assert!(matches!(third, LoopGuardDecision::AllowWithWarning { .. }));
    }

    #[test]
    fn different_path_field_targets_are_not_duplicate() {
        let mut state = LoopGuardState::new(LoopGuardConfig::default());
        let first = build_tool_call_signature("Read", &json!({"path":"a.rs","offset":1,"limit":100}));
        let second = build_tool_call_signature("Read", &json!({"path":"b.rs","offset":1,"limit":100}));

        let _ = state.evaluate_tool_call(first);
        let decision = state.evaluate_tool_call(second);
        assert!(matches!(decision, LoopGuardDecision::Allow));
    }

    #[test]
    fn disabled_guard_allows_duplicate_and_stall() {
        let mut state = LoopGuardState::new(LoopGuardConfig {
            enabled: false,
            ..LoopGuardConfig::default()
        });
        let sig = build_tool_call_signature("Read", &json!({"file_path":"a.rs","offset":1,"limit":100}));
        let first = state.evaluate_tool_call(sig.clone());
        let second = state.evaluate_tool_call(sig);
        assert!(matches!(first, LoopGuardDecision::Allow));
        assert!(matches!(second, LoopGuardDecision::Allow));

        let fp = assistant_fingerprint_from_text("same");
        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(!state.detect_stalled_iteration(fp.clone(), 1));
        assert!(!state.detect_stalled_iteration(fp, 1));
    }

    #[test]
    fn assistant_fingerprint_distinguishes_suffix_with_same_prefix_and_len() {
        let prefix = "a".repeat(64);
        let left = format!("{}{}", prefix, "left");
        let right = format!("{}{}", prefix, "rigt");
        assert_eq!(left.chars().count(), right.chars().count());

        let left_fp = assistant_fingerprint_from_text(&left);
        let right_fp = assistant_fingerprint_from_text(&right);
        assert_ne!(left_fp, right_fp);
    }

    #[test]
    fn loop_guard_config_supports_serde_round_trip() {
        let cfg = LoopGuardConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize loop guard config");
        let de: LoopGuardConfig = serde_json::from_str(&json).expect("deserialize loop guard config");
        assert_eq!(cfg.enabled, de.enabled);
        assert_eq!(
            cfg.max_consecutive_duplicate_tool_calls,
            de.max_consecutive_duplicate_tool_calls
        );
        assert_eq!(cfg.max_stalled_iterations, de.max_stalled_iterations);
        assert_eq!(cfg.duplicate_read_mode, de.duplicate_read_mode);
        assert_eq!(cfg.iteration_trim_ratio, de.iteration_trim_ratio);
    }
}
