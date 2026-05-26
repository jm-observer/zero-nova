use crate::tool::ToolOutput;
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn validate_input_against_schema(tool_name: &str, input: &Value, schema: &Value) -> Result<(), ToolOutput> {
    let Some(input_obj) = input.as_object() else {
        return Err(ToolOutput {
            content: format!("Invalid arguments for '{}': input must be a JSON object", tool_name),
            is_error: true,
            child_session: None,
        });
    };

    let schema_props = schema.get("properties").and_then(Value::as_object);
    let schema_required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| required.iter().filter_map(Value::as_str).collect::<HashSet<_>>())
        .unwrap_or_default();

    if let Some(props) = schema_props {
        for key in input_obj.keys() {
            if !props.contains_key(key) {
                return Err(ToolOutput {
                    content: format!("Invalid arguments for '{}': unknown field '{}'", tool_name, key),
                    is_error: true,
                    child_session: None,
                });
            }
        }

        for required in &schema_required {
            if !input_obj.contains_key(*required) {
                return Err(ToolOutput {
                    content: format!(
                        "Invalid arguments for '{}': missing required field '{}'",
                        tool_name, required
                    ),
                    is_error: true,
                    child_session: None,
                });
            }
        }

        for (key, value) in input_obj {
            if let Some(prop_schema) = props.get(key) {
                if let Some(expected_type) = prop_schema.get("type").and_then(Value::as_str) {
                    let type_ok = match expected_type {
                        "string" => value.is_string(),
                        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
                        "number" => value.is_number(),
                        "boolean" => value.is_boolean(),
                        "object" => value.is_object(),
                        "array" => value.is_array(),
                        _ => true,
                    };
                    if !type_ok {
                        return Err(ToolOutput {
                            content: format!(
                                "Invalid arguments for '{}': field '{}' must be type '{}'",
                                tool_name, key, expected_type
                            ),
                            is_error: true,
                            child_session: None,
                        });
                    }
                }
            }
        }
    }

    Ok(())
}
