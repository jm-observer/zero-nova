use std::fs;
use std::path::PathBuf;

use nova_protocol::GatewayMessage;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/fixtures")
        .join(name)
}

fn load_fixture(name: &str) -> String {
    fs::read_to_string(fixture_path(name)).unwrap()
}

#[test]
fn valid_contract_fixtures_deserialize() {
    let fixtures = [
        "agent_inspect.json",
        "welcome.json",
        "error.json",
        "chat.json",
        "chat_complete.json",
        "skill_activated.json",
        "task_status_changed.json",
        "progress_event.json",
        "workspace_restore.json",
    ];

    for fixture in fixtures {
        let raw = load_fixture(fixture);
        let parsed = serde_json::from_str::<GatewayMessage>(&raw);
        assert!(parsed.is_ok(), "fixture should deserialize: {fixture}");
    }
}

#[test]
fn workspace_restore_fixture_allows_empty_payload() {
    let raw = load_fixture("workspace_restore.json");
    let parsed = serde_json::from_str::<GatewayMessage>(&raw).unwrap();
    let serialized = serde_json::to_value(parsed).unwrap();
    assert!(serialized["payload"].is_object());
    assert!(serialized["payload"]["userId"].is_null());
}

#[test]
fn invalid_contract_fixtures_fail_deserialize() {
    for fixture in [
        "invalid_agent_inspect_missing_session_id.json",
        "invalid_chat_missing_input.json",
        "invalid_error_missing_code.json",
        "invalid_welcome_missing_optional_field.json",
        "invalid_workspace_restore_missing_payload.json",
    ] {
        let raw = load_fixture(fixture);
        let parsed = serde_json::from_str::<GatewayMessage>(&raw);
        assert!(parsed.is_err(), "fixture should fail: {fixture}");
    }
}
