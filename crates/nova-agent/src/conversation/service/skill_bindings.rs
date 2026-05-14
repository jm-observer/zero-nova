use std::collections::HashMap;

pub(super) fn merge_skill_bindings(existing: &mut Vec<serde_json::Value>, incoming: Vec<serde_json::Value>) {
    let mut merged: HashMap<String, serde_json::Value> = HashMap::new();
    for skill in existing.iter() {
        if let Some(skill_id) = skill.get("skill_id").and_then(|v| v.as_str()) {
            merged.insert(skill_id.to_string(), normalize_skill_binding(skill));
        }
    }

    for skill in incoming {
        if let Some(skill_id) = skill.get("skill_id").and_then(|v| v.as_str()) {
            merged.insert(skill_id.to_string(), normalize_skill_binding(&skill));
        } else {
            log::warn!("Skipping invalid skill binding item without skill_id: {}", skill);
        }
    }

    *existing = merged.into_values().collect();
}

fn normalize_skill_binding(skill: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "skill_id": skill.get("skill_id").and_then(|v| v.as_str()).unwrap_or_default(),
        "name": skill.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
        "status": skill.get("status").and_then(|v| v.as_str()).unwrap_or_default(),
        "description": skill.get("description").cloned().unwrap_or(serde_json::Value::Null),
    })
}
