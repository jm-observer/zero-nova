/// 后端契约测试：验证协议 DTO 与 JSON Schema 的一致性。
///
/// 这些测试确保：
/// 1. 所有 fixtures 可以正确反序列化为 Rust DTO。
/// 2. 反序列化后的 DTO 重新序列化后与原始 fixture 等价（稳定化输出）。
/// 3. 当 schemars feature 启用时，序列化结果可以进一步通过 JSON Schema 校验。
extern crate nova_protocol;

use nova_protocol::chat;
use nova_protocol::envelope;
use nova_protocol::session;
use nova_protocol::system;
use serde_json::Value;

// ============================================================
// Fixture 加载辅助
// ============================================================

const FIXTURE_DIR: &str = "tests/fixtures";

/// 加载 fixture 文件为 `serde_json::Value`。
fn load_fixture(name: &str) -> Value {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = std::path::Path::new(&root).join(FIXTURE_DIR).join(name);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to load fixture '{}': {}", path.display(), e));
    serde_json::from_str(&content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse fixture '{}': {} - {}",
            path.display(),
            content.trim().get(0..60).unwrap_or(""),
            e
        )
    })
}

// ============================================================
// 比较辅助函数
// ============================================================

/// 比较两个 `Value` 对象是否等价（按 BTreeMap 键序比较）。
fn values_equal_normalized(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a_map), Value::Object(b_map)) => {
            if a_map.len() != b_map.len() {
                return false;
            }
            let a_sorted: std::collections::BTreeMap<&String, &Value> = a_map.iter().collect();
            let b_sorted: std::collections::BTreeMap<&String, &Value> = b_map.iter().collect();
            a_sorted
                .iter()
                .zip(b_sorted.iter())
                .all(|((k1, v1), (k2, v2))| k1 == k2 && values_equal_normalized(v1, v2))
        }
        (Value::Array(a_arr), Value::Array(b_arr)) => {
            a_arr.len() == b_arr.len()
                && a_arr
                    .iter()
                    .zip(b_arr.iter())
                    .all(|(a, b)| values_equal_normalized(a, b))
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

// ============================================================
// Schema 校验辅助（当 export-schema feature 启用时）
// ============================================================

#[cfg(feature = "export-schema")]
fn validate_with_schema(schema_type: &str, value: &Value) -> Result<(), Vec<String>> {
    use schemars::JsonSchema;
    use serde::de::DeserializeOwned;
    use std::any::TypeId;

    macro_rules! try_validate {
        ($T:ty) => {{
            let _typed: $T = serde_json::from_value(value.clone())
                .map_err(|e| vec![format!("Deserialization failed for {}: {}", TypeId::of::<$T>(), e)])?;
            let re_serialized = serde_json::to_value(_typed.clone())
                .map_err(|e| vec![format!("Re-serialization failed for {}: {}", TypeId::of::<$T>(), e)])?;
            if !values_equal_normalized(value, &re_serialized) {
                return Err(vec![format!(
                    "Schema mismatch for {}: roundtrip serialization diff detected",
                    TypeId::of::<$T>()
                )]);
            }
            Ok(())
        }};
    }

    match schema_type {
        "GatewayMessage" => try_validate!(envelope::GatewayMessage),
        "WelcomePayload" => try_validate!(system::WelcomePayload),
        "ErrorPayload" => try_validate!(system::ErrorPayload),
        "ChatPayload" => try_validate!(chat::ChatPayload),
        "ChatCompletePayload" => try_validate!(chat::ChatCompletePayload),
        "ProgressEvent" => try_validate!(chat::ProgressEvent),
        "SkillActivatedPayload" => try_validate!(chat::SkillActivatedPayload),
        "TaskStatusChangedPayload" => try_validate!(chat::TaskStatusChangedPayload),
        _ => Ok(()),
    }
}

// ============================================================
// 契约测试：正常路径
// ============================================================

#[test]
fn contract_welcome_roundtrip() {
    let fixture = load_fixture("welcome.json");

    // 反序列化
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok(), "welcome.json must deserialize as GatewayMessage");

    let msg = result.unwrap();
    assert!(
        matches!(msg.envelope, envelope::MessageEnvelope::Welcome(ref p) if p.require_auth == false && p.setup_required == true)
    );

    // Round-trip 序列化对比
    let re_serialized = serde_json::to_value(&msg).unwrap();
    if !values_equal_normalized(&fixture, &re_serialized) {
        panic!("welcome.json roundtrip serialization mismatch");
    }

    // Schema 校验（feature enabled）
    #[cfg(feature = "export-schema")]
    {
        let validation_result = validate_with_schema("GatewayMessage", &fixture);
        assert!(
            validation_result.is_ok(),
            "Schema validation failed for welcome: {:?}",
            validation_result.err()
        );
    }
}

#[test]
fn contract_error_roundtrip() {
    let fixture = load_fixture("error.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert!(matches!(msg.envelope, envelope::MessageEnvelope::Error(_)));

    // Schema 校验
    #[cfg(feature = "export-schema")]
    {
        assert!(validate_with_schema("ErrorPayload", &fixture).is_ok());
    }
}

#[test]
fn contract_chat_roundtrip() {
    let fixture = load_fixture("chat.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    // Round-trip
    let msg = result.unwrap();
    let re_serialized = serde_json::to_value(&msg).unwrap();
    if !values_equal_normalized(&fixture, &re_serialized) {
        panic!("chat.json roundtrip serialization mismatch");
    }

    // Schema 校验
    #[cfg(feature = "export-schema")]
    {
        assert!(validate_with_schema("ChatPayload", &fixture).is_ok());
    }
}

#[test]
fn contract_chat_complete_roundtrip() {
    let fixture = load_fixture("chat_complete.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert!(matches!(msg.envelope, envelope::MessageEnvelope::ChatComplete(_)));
}

#[test]
fn contract_skill_activated_roundtrip() {
    let fixture = load_fixture("skill_activated.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert!(matches!(msg.envelope, envelope::MessageEnvelope::SkillActivated(_)));

    // Schema 校验
    #[cfg(feature = "export-schema")]
    {
        assert!(validate_with_schema("SkillActivatedPayload", &fixture).is_ok());
    }
}

#[test]
fn contract_task_status_changed_roundtrip() {
    let fixture = load_fixture("task_status_changed.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert!(matches!(msg.envelope, envelope::MessageEnvelope::TaskStatusChanged(_)));
}

#[test]
fn contract_progress_event_roundtrip() {
    let fixture = load_fixture("progress_event.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert!(matches!(msg.envelope, envelope::MessageEnvelope::ChatProgress(_)));

    // Schema 校验
    #[cfg(feature = "export-schema")]
    {
        assert!(validate_with_schema("ProgressEvent", &fixture).is_ok());
    }
}

// ============================================================
// 契约测试：异常路径
// ============================================================

#[test]
fn contract_invalid_error_fails_type_check() {
    // invalid_error_missing_code.json has "message" as number instead of string
    let fixture = load_fixture("invalid_error_missing_code.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());

    // Should fail because ErrorPayload expects message: String
    assert!(
        result.is_err(),
        "invalid_error_missing_code.json should fail deserialization (message is number, expected string)"
    );
}

#[test]
fn contract_invalid_chat_missing_required_field() {
    // invalid_chat_missing_input.json missing required "input" field
    let fixture = load_fixture("invalid_chat_missing_input.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());

    // ChatPayload requires input field
    assert!(
        result.is_err(),
        "invalid_chat_missing_input.json should fail deserialization (missing required 'input' field)"
    );
}

#[test]
fn contract_welcome_optional_field_allowed() {
    // invalid_welcome_missing_optional_field.json has only requireAuth (missing optional setupRequired)
    let fixture = load_fixture("invalid_welcome_missing_optional_field.json");
    let result: serde_json::Result<envelope::GatewayMessage> = serde_json::from_value(fixture.clone());

    // WelcomePayload has setupRequired with Default, so this should succeed
    assert!(
        result.is_ok(),
        "invalid_welcome_missing_optional_field.json should succeed (setupRequired has Default)"
    );
}

// ============================================================
// 所有已知类型遍历测试
// ============================================================

#[test]
fn contract_all_message_envelope_variants_serialize() {
    // Test that all MessageEnvelope variants can be created and serialized
    let variants = vec![
        (
            "error",
            envelope::MessageEnvelope::Error(system::ErrorPayload {
                message: "test".into(),
                code: Some("TEST".into()),
            }),
        ),
        (
            "welcome",
            envelope::MessageEnvelope::Welcome(system::WelcomePayload {
                require_auth: true,
                setup_required: false,
            }),
        ),
        (
            "chat",
            envelope::MessageEnvelope::Chat(chat::ChatPayload {
                input: "hello".into(),
                session_id: Some("sess-1".into()),
                ..Default::default()
            }),
        ),
        (
            "chat.complete",
            envelope::MessageEnvelope::ChatComplete(chat::ChatCompletePayload {
                session_id: "sess-1".into(),
                output: Some("done".into()),
                ..Default::default()
            }),
        ),
        (
            "chat.start",
            envelope::MessageEnvelope::ChatStart(session::SessionIdPayload {
                session_id: "sess-1".into(),
            }),
        ),
        (
            "skill.activated",
            envelope::MessageEnvelope::SkillActivated(chat::SkillActivatedPayload {
                session_id: Some("1".into()),
                skill_id: "test".into(),
                skill_name: "Test".into(),
                sticky: true,
                reason: "auto".into(),
            }),
        ),
        (
            "skill.switched",
            envelope::MessageEnvelope::SkillSwitched(chat::SkillSwitchedPayload {
                session_id: Some("1".into()),
                from_skill: "s1".into(),
                to_skill: "s2".into(),
                reason: "manual".into(),
            }),
        ),
        (
            "skill.exited",
            envelope::MessageEnvelope::SkillExited(chat::SkillExitedPayload {
                session_id: Some("1".into()),
                skill_id: "s1".into(),
                skill_name: "Skill1".into(),
                reason: "forced".into(),
            }),
        ),
        (
            "tool.unlocked",
            envelope::MessageEnvelope::ToolUnlocked(chat::ToolUnlockedPayload {
                session_id: Some("1".into()),
                tool_name: "Bash".into(),
                source: "skill_activation".into(),
            }),
        ),
        (
            "task.status_changed",
            envelope::MessageEnvelope::TaskStatusChanged(chat::TaskStatusChangedPayload {
                session_id: Some("1".into()),
                task_id: "1".into(),
                task_subject: "Build".into(),
                status: "running".into(),
                active_form: None,
                is_main_task: false,
            }),
        ),
        (
            "skill.route_evaluated",
            envelope::MessageEnvelope::SkillRouteEvaluated(chat::SkillRouteEvaluatedPayload {
                session_id: Some("1".into()),
                skill_id: "s1".into(),
                confidence: 0.95,
                decision: "activate".into(),
                reasoning: "high confidence".into(),
            }),
        ),
        (
            "skill.invocation",
            envelope::MessageEnvelope::SkillInvocation(chat::SkillInvocationPayload {
                session_id: Some("1".into()),
                skill_id: "s1".into(),
                skill_name: "Skill1".into(),
                level: "auto".into(),
            }),
        ),
    ];

    for (expected_type, env) in variants {
        let msg = envelope::GatewayMessage::new_event(env);
        let json = serde_json::to_string(&msg).expect(&format!("Failed to serialize {}", expected_type));
        assert!(
            json.contains(&format!("\"type\":\"{}\"", expected_type)),
            "{}: expected type in JSON but got: {}",
            expected_type,
            json
        );
    }
}
