use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::{schema_for, JsonSchema};
use serde_json::{Map, Value};

use crate::chat::{ChatCompletePayload, ChatPayload, ProgressEvent};
use crate::envelope::{GatewayMessage, MessageEnvelope};
use crate::observability::{
    AgentInspectRequest, AgentInspectResponse, WorkspaceRestoreRequest, WorkspaceRestoreResponse,
};
use crate::session::{SessionCreateRequest, SessionCreateResponse, SessionIdPayload};
use crate::system::{ErrorPayload, WelcomePayload};

const JSON_SCHEMA_DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

const FIXTURES: &[(&str, &str)] = &[
    ("chat.json", include_str!("../../../schemas/fixtures/chat.json")),
    (
        "chat_complete.json",
        include_str!("../../../schemas/fixtures/chat_complete.json"),
    ),
    ("error.json", include_str!("../../../schemas/fixtures/error.json")),
    (
        "invalid_chat_missing_input.json",
        include_str!("../../../schemas/fixtures/invalid_chat_missing_input.json"),
    ),
    (
        "invalid_error_missing_code.json",
        include_str!("../../../schemas/fixtures/invalid_error_missing_code.json"),
    ),
    (
        "invalid_welcome_missing_optional_field.json",
        include_str!("../../../schemas/fixtures/invalid_welcome_missing_optional_field.json"),
    ),
    (
        "progress_event.json",
        include_str!("../../../schemas/fixtures/progress_event.json"),
    ),
    (
        "skill_activated.json",
        include_str!("../../../schemas/fixtures/skill_activated.json"),
    ),
    (
        "task_status_changed.json",
        include_str!("../../../schemas/fixtures/task_status_changed.json"),
    ),
    ("welcome.json", include_str!("../../../schemas/fixtures/welcome.json")),
];

struct SchemaArtifactSpec {
    type_name: &'static str,
    domain: &'static str,
    file_name: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    build: fn() -> Result<Value>,
}

struct WrittenArtifact {
    type_name: &'static str,
    title: &'static str,
    domain: &'static str,
    description: Option<&'static str>,
    relative_path: PathBuf,
}

macro_rules! schema_builder {
    ($fn_name:ident, $ty:ty, $title:literal) => {
        fn $fn_name() -> Result<Value> {
            build_schema::<$ty>($title, None)
        }
    };
    ($fn_name:ident, $ty:ty, $title:literal, $description:literal) => {
        fn $fn_name() -> Result<Value> {
            build_schema::<$ty>($title, Some($description))
        }
    };
}

schema_builder!(
    gateway_message_schema,
    GatewayMessage,
    "GatewayMessage",
    "Gateway websocket message root."
);
schema_builder!(
    message_envelope_schema,
    MessageEnvelope,
    "MessageEnvelope",
    "Tagged gateway envelope payload."
);
schema_builder!(chat_payload_schema, ChatPayload, "ChatPayload");
schema_builder!(chat_complete_payload_schema, ChatCompletePayload, "ChatCompletePayload");
schema_builder!(progress_event_schema, ProgressEvent, "ProgressEvent");
schema_builder!(
    session_create_request_schema,
    SessionCreateRequest,
    "SessionCreateRequest"
);
schema_builder!(
    session_create_response_schema,
    SessionCreateResponse,
    "SessionCreateResponse"
);
schema_builder!(session_id_payload_schema, SessionIdPayload, "SessionIdPayload");
schema_builder!(agent_inspect_request_schema, AgentInspectRequest, "AgentInspectRequest");
schema_builder!(
    agent_inspect_response_schema,
    AgentInspectResponse,
    "AgentInspectResponse"
);
schema_builder!(
    workspace_restore_request_schema,
    WorkspaceRestoreRequest,
    "WorkspaceRestoreRequest"
);
schema_builder!(
    workspace_restore_response_schema,
    WorkspaceRestoreResponse,
    "WorkspaceRestoreResponse"
);
schema_builder!(welcome_payload_schema, WelcomePayload, "WelcomePayload");
schema_builder!(error_payload_schema, ErrorPayload, "ErrorPayload");

const ROOT_SCHEMA_ARTIFACTS: &[SchemaArtifactSpec] = &[
    SchemaArtifactSpec {
        type_name: "GatewayMessage",
        domain: "gateway",
        file_name: "gateway-message.schema.json",
        title: "GatewayMessage",
        description: Some("Gateway websocket message root."),
        build: gateway_message_schema,
    },
    SchemaArtifactSpec {
        type_name: "MessageEnvelope",
        domain: "gateway",
        file_name: "message-envelope.schema.json",
        title: "MessageEnvelope",
        description: Some("Tagged gateway envelope payload."),
        build: message_envelope_schema,
    },
    SchemaArtifactSpec {
        type_name: "ChatPayload",
        domain: "chat",
        file_name: "chat-payload.schema.json",
        title: "ChatPayload",
        description: None,
        build: chat_payload_schema,
    },
    SchemaArtifactSpec {
        type_name: "ChatCompletePayload",
        domain: "chat",
        file_name: "chat-complete-payload.schema.json",
        title: "ChatCompletePayload",
        description: None,
        build: chat_complete_payload_schema,
    },
    SchemaArtifactSpec {
        type_name: "ProgressEvent",
        domain: "chat",
        file_name: "progress-event.schema.json",
        title: "ProgressEvent",
        description: None,
        build: progress_event_schema,
    },
    SchemaArtifactSpec {
        type_name: "SessionCreateRequest",
        domain: "session",
        file_name: "session-create-request.schema.json",
        title: "SessionCreateRequest",
        description: None,
        build: session_create_request_schema,
    },
    SchemaArtifactSpec {
        type_name: "SessionCreateResponse",
        domain: "session",
        file_name: "session-create-response.schema.json",
        title: "SessionCreateResponse",
        description: None,
        build: session_create_response_schema,
    },
    SchemaArtifactSpec {
        type_name: "SessionIdPayload",
        domain: "session",
        file_name: "session-id-payload.schema.json",
        title: "SessionIdPayload",
        description: None,
        build: session_id_payload_schema,
    },
    SchemaArtifactSpec {
        type_name: "AgentInspectRequest",
        domain: "observability",
        file_name: "agent-inspect-request.schema.json",
        title: "AgentInspectRequest",
        description: None,
        build: agent_inspect_request_schema,
    },
    SchemaArtifactSpec {
        type_name: "AgentInspectResponse",
        domain: "observability",
        file_name: "agent-inspect-response.schema.json",
        title: "AgentInspectResponse",
        description: None,
        build: agent_inspect_response_schema,
    },
    SchemaArtifactSpec {
        type_name: "WorkspaceRestoreRequest",
        domain: "observability",
        file_name: "workspace-restore-request.schema.json",
        title: "WorkspaceRestoreRequest",
        description: None,
        build: workspace_restore_request_schema,
    },
    SchemaArtifactSpec {
        type_name: "WorkspaceRestoreResponse",
        domain: "observability",
        file_name: "workspace-restore-response.schema.json",
        title: "WorkspaceRestoreResponse",
        description: None,
        build: workspace_restore_response_schema,
    },
    SchemaArtifactSpec {
        type_name: "WelcomePayload",
        domain: "system",
        file_name: "welcome-payload.schema.json",
        title: "WelcomePayload",
        description: None,
        build: welcome_payload_schema,
    },
    SchemaArtifactSpec {
        type_name: "ErrorPayload",
        domain: "system",
        file_name: "error-payload.schema.json",
        title: "ErrorPayload",
        description: None,
        build: error_payload_schema,
    },
];

pub fn export_repository_artifacts(root: &Path) -> Result<()> {
    sync_shared_fixtures(root)?;
    let artifacts = export_schema_artifacts(root)?;
    write_registry(root, &artifacts)?;
    write_schema_root(root, &artifacts)?;
    write_domain_snapshot(root)?;
    Ok(())
}

fn export_schema_artifacts(root: &Path) -> Result<Vec<WrittenArtifact>> {
    let schemas_root = root.join("schemas");
    fs::create_dir_all(schemas_root.join("domains"))
        .with_context(|| format!("创建 schema 目录失败: {}", schemas_root.display()))?;
    fs::create_dir_all(schemas_root.join("root"))
        .with_context(|| format!("创建 root schema 目录失败: {}", schemas_root.join("root").display()))?;

    prune_stale_domain_schemas(root)?;

    let mut artifacts = Vec::with_capacity(ROOT_SCHEMA_ARTIFACTS.len());
    for spec in ROOT_SCHEMA_ARTIFACTS {
        let schema_value = (spec.build)().with_context(|| format!("生成 {} schema 失败", spec.title))?;
        assert_schema_is_json(&schema_value, spec.title)?;

        let relative_path = PathBuf::from("schemas")
            .join("domains")
            .join(spec.domain)
            .join(spec.file_name);
        let absolute_path = root.join(&relative_path);

        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }

        write_if_changed(&absolute_path, &render_json(&schema_value)?)?;
        artifacts.push(WrittenArtifact {
            type_name: spec.type_name,
            title: spec.title,
            domain: spec.domain,
            description: spec.description,
            relative_path,
        });
    }

    Ok(artifacts)
}

fn build_schema<T: JsonSchema>(title: &'static str, description: Option<&'static str>) -> Result<Value> {
    let mut value = serde_json::to_value(schema_for!(T)).context("序列化 RootSchema 失败")?;
    let object = value.as_object_mut().context("schema 根节点不是对象")?;

    object.insert("$schema".to_string(), Value::String(JSON_SCHEMA_DRAFT.to_string()));
    object.insert("title".to_string(), Value::String(title.to_string()));
    if let Some(description) = description {
        object.insert("description".to_string(), Value::String(description.to_string()));
    }
    object
        .entry("$defs".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    Ok(value)
}

fn sync_shared_fixtures(root: &Path) -> Result<()> {
    let fixture_dir = root.join("schemas").join("fixtures");
    fs::create_dir_all(&fixture_dir).with_context(|| format!("创建 fixture 目录失败: {}", fixture_dir.display()))?;

    for (name, content) in FIXTURES {
        let path = fixture_dir.join(name);
        write_if_changed(&path, content)?;
    }

    Ok(())
}

fn prune_stale_domain_schemas(root: &Path) -> Result<()> {
    let expected_paths = ROOT_SCHEMA_ARTIFACTS
        .iter()
        .map(|spec| {
            PathBuf::from("schemas")
                .join("domains")
                .join(spec.domain)
                .join(spec.file_name)
        })
        .collect::<BTreeSet<_>>();

    let domains_dir = root.join("schemas").join("domains");
    if !domains_dir.exists() {
        return Ok(());
    }

    for file in collect_schema_files(&domains_dir)? {
        let relative = file
            .strip_prefix(root)
            .with_context(|| format!("计算相对路径失败: {}", file.display()))?
            .to_path_buf();

        if !expected_paths.contains(&relative) {
            fs::remove_file(&file).with_context(|| format!("删除过期 schema 失败: {}", file.display()))?;
        }
    }

    Ok(())
}

fn write_registry(root: &Path, artifacts: &[WrittenArtifact]) -> Result<()> {
    let mut types = Map::new();
    let mut domain_roots = BTreeMap::new();

    for artifact in artifacts {
        let mut type_entry = Map::new();
        type_entry.insert("title".to_string(), Value::String(artifact.title.to_string()));
        type_entry.insert("domain".to_string(), Value::String(artifact.domain.to_string()));
        type_entry.insert(
            "path".to_string(),
            Value::String(normalize_path(&artifact.relative_path)),
        );

        if let Some(description) = artifact.description {
            type_entry.insert("description".to_string(), Value::String(description.to_string()));
        }

        types.insert(artifact.type_name.to_string(), Value::Object(type_entry));
        domain_roots.insert(
            artifact.domain.to_string(),
            normalize_path(&PathBuf::from("schemas").join("domains").join(artifact.domain)),
        );
    }

    let registry = serde_json::json!({
        "metadata": {
            "sourceCrate": "nova-protocol",
            "sourceCrateVersion": env!("CARGO_PKG_VERSION"),
            "jsonSchemaDraft": JSON_SCHEMA_DRAFT,
        },
        "types": types,
        "domainRoots": domain_roots,
    });

    write_if_changed(&root.join("schemas/registry.json"), &render_json(&registry)?)
}

fn write_schema_root(root: &Path, artifacts: &[WrittenArtifact]) -> Result<()> {
    let mut grouped = BTreeMap::<&str, Vec<&WrittenArtifact>>::new();
    for artifact in artifacts {
        grouped.entry(artifact.domain).or_default().push(artifact);
    }

    let mut domains = Map::new();
    for (domain, domain_artifacts) in grouped {
        let mut properties = Map::new();
        for artifact in domain_artifacts {
            properties.insert(
                artifact.type_name.to_string(),
                serde_json::json!({ "$ref": format!("../{}", normalize_path(&artifact.relative_path)) }),
            );
        }

        domains.insert(
            domain.to_string(),
            Value::Object(Map::from_iter([
                ("type".to_string(), Value::String("object".to_string())),
                ("properties".to_string(), Value::Object(properties)),
            ])),
        );
    }

    let root_schema = serde_json::json!({
        "$schema": JSON_SCHEMA_DRAFT,
        "title": "SchemaRoot",
        "description": "Root schema referencing exported nova-protocol domain schemas.",
        "type": "object",
        "properties": domains,
        "$defs": {},
    });

    write_if_changed(&root.join("schemas/root/schema-root.json"), &render_json(&root_schema)?)
}

fn write_domain_snapshot(root: &Path) -> Result<()> {
    let domains_dir = root.join("schemas").join("domains");
    let mut files = collect_schema_files(&domains_dir)?;
    files.sort();

    let snapshot = files
        .into_iter()
        .map(|path| normalize_path(path.strip_prefix(root).unwrap_or(path.as_path())))
        .collect::<Vec<_>>()
        .join("\n");

    write_if_changed(&root.join("schemas/domains_snapshot.txt"), &format!("{}\n", snapshot))
}

fn collect_schema_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir).with_context(|| format!("读取目录失败: {}", dir.display()))? {
        let entry = entry.with_context(|| format!("读取目录项失败: {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(collect_schema_files(&path)?);
            continue;
        }

        let is_schema = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".schema.json"))
            .unwrap_or(false);

        if is_schema {
            files.push(path);
        }
    }

    Ok(files)
}

fn render_json(value: &Value) -> Result<String> {
    let mut rendered = serde_json::to_string_pretty(value).context("格式化 JSON 失败")?;
    rendered.push('\n');
    Ok(rendered)
}

fn assert_schema_is_json(schema: &Value, title: &str) -> Result<()> {
    serde_json::to_string(schema).with_context(|| format!("{title} schema 不是合法 JSON"))?;
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if should_write {
        fs::write(path, content).with_context(|| format!("写入文件失败: {}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn export_writes_required_roots() {
        let root = create_temp_root("schema-export");
        export_repository_artifacts(&root).unwrap();

        assert!(root.join("schemas/registry.json").is_file());
        assert!(root.join("schemas/root/schema-root.json").is_file());
        assert!(root
            .join("schemas/domains/gateway/gateway-message.schema.json")
            .is_file());
        assert!(root
            .join("schemas/domains/gateway/message-envelope.schema.json")
            .is_file());
        assert!(root
            .join("schemas/domains/observability/agent-inspect-request.schema.json")
            .is_file());
        assert!(root
            .join("schemas/domains/observability/workspace-restore-request.schema.json")
            .is_file());

        let registry = read_json(&root.join("schemas/registry.json"));
        assert!(registry["types"].get("GatewayMessage").is_some());
        assert!(registry["types"].get("MessageEnvelope").is_some());
        assert!(registry["types"].get("AgentInspectRequest").is_some());
        assert!(registry["types"].get("WorkspaceRestoreRequest").is_some());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_inspect_schema_requires_session_id_and_agent_id() {
        let root = create_temp_root("schema-agent-inspect");
        export_repository_artifacts(&root).unwrap();

        let schema = read_json(&root.join("schemas/domains/observability/agent-inspect-request.schema.json"));
        let required = schema["required"].as_array().unwrap();

        assert!(required.iter().any(|value| value == "sessionId"));
        assert!(required.iter().any(|value| value == "agentId"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_restore_envelope_requires_payload() {
        let root = create_temp_root("schema-workspace-restore");
        export_repository_artifacts(&root).unwrap();

        let schema = read_json(&root.join("schemas/domains/gateway/message-envelope.schema.json"));
        let variants = schema["oneOf"].as_array().unwrap();
        let workspace_restore = variants
            .iter()
            .find(|variant| {
                variant["properties"]["type"]["const"] == "workspace.restore"
                    || variant["properties"]["type"]["enum"]
                        .as_array()
                        .map(|values| values.iter().any(|value| value == "workspace.restore"))
                        .unwrap_or(false)
            })
            .unwrap();
        let required = workspace_restore["required"].as_array().unwrap();

        assert!(required.iter().any(|value| value == "payload"));

        fs::remove_dir_all(root).unwrap();
    }

    fn create_temp_root(prefix: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("zero-nova-{prefix}-{unique}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn read_json(path: &Path) -> Value {
        let content = fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    }
}
