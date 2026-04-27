/// Schema 导出模块：使用 schemars 将 Rust 协议类型生成 JSON Schema 文件。
///
/// 该模块提供完整的 Schema 导出流水线：
/// 1. 遍历所有标记为导出的协议类型
/// 2. 使用 `schemars::schema_for!` 生成 JSON Schema
/// 3. 按域名组织到 `schemas/domains/` 目录
/// 4. 生成根 registry 文件
#[cfg(feature = "export-schema")]
use schemars::{schema_for, JsonSchema};
use serde::Serialize;
#[cfg(feature = "export-schema")]
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

// ============================================================
// Schema 导出配置
// ============================================================

/// Schema 工件输出根目录（相对于仓库根）。
pub const SCHEMA_OUTPUT_DIR: &str = "schemas";

/// 域名 -> Schema 文件路径映射。
pub const DOMAIN_PATHS: &[(&str, &str)] = &[
    ("gateway", "domains/gateway"),
    ("chat", "domains/chat"),
    ("session", "domains/session"),
    ("agent", "domains/agent"),
    ("system", "domains/system"),
    ("observability", "domains/observability"),
    ("skills", "domains/skills"),
];

/// Schema 元数据，写入每个生成的文件。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFileMetadata {
    pub generated_at: String,
    pub source_crate: String,
    pub source_crate_version: String,
}

// ============================================================
// Schema 类型注册表
// ============================================================

/// 使用 `BTreeMap` 保持键序一致性，确保每次生成输出相同。
#[cfg(feature = "export-schema")]
pub type TypeRegistry = BTreeMap<String, (String, serde_json::Value)>;

/// 返回 Schema 导出引用的域列表。
pub fn get_export_domains() -> Vec<String> {
    DOMAIN_PATHS.iter().map(|(name, _)| (*name).to_string()).collect()
}

// ============================================================
// Schema 导出核心逻辑
// ============================================================

/// 生成指定类型的 JSON Schema JSON 字符串（稳定化输出）。
#[cfg(feature = "export-schema")]
pub fn generate_schema_json<T: JsonSchema>(title: Option<&str>) -> String {
    let mut schema = schema_for!(T);

    // 添加可选的标题
    if let Some(t) = title {
        // schemars v1 schema 使用 `schema.title = ...` 兼容旧 API
        let s: &mut schemars::Schema = &mut schema;
        unsafe {
            let obj = &mut *(s as *const schemars::Schema as *mut serde_json::Value);
            if let serde_json::Value::Object(map) = obj {
                map.insert("title".to_string(), serde_json::Value::String(t.to_string()));
            }
        }
    }

    // 稳定化输出：递归排序对象的所有键
    let mut metadata = serde_json::to_value(&schema).unwrap();
    sort_object_recursive(&mut metadata);

    serde_json::to_string_pretty(&metadata).unwrap()
}

/// 递归排序对象的所有键（确保键序稳定）。
#[cfg(feature = "export-schema")]
fn sort_object_recursive(obj: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = obj {
        let sorted_keys: Vec<_> = map.keys().cloned().collect();
        for key in sorted_keys {
            if let Some(item) = map.get_mut(&key) {
                sort_object_recursive(item);
            }
        }
        *obj = serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<String, serde_json::Value>>()
                .into_iter()
                .collect(),
        );
    }
}

/// 导出根 Schema 引用文件。
pub fn generate_root_schema(output_dir: &Path, metadata: &SchemaFileMetadata) -> Result<(), std::io::Error> {
    let mut domain_refs = serde_json::Map::new();
    for (name, path) in DOMAIN_PATHS {
        let rel_path = path.trim_start_matches("domains/");
        domain_refs.insert(
            name.to_string(),
            serde_json::json!({
                "$ref": &format!("../{}/{}.json", rel_path, name)
            }),
        );
    }

    let root_schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://zero-nova.org/schemas/root/schema-root.json",
        "title": "Zero-Nova Protocol Schema Registry",
        "description": "Root schema referencing all domain schemas. Generated from nova-protocol Rust types.",
        "type": "object",
        "version": {
            "major": 1,
            "minor": 0,
            "patch": 0
        },
        "generatedAt": metadata.generated_at,
        "sourceCrate": metadata.source_crate,
        "sourceCrateVersion": metadata.source_crate_version,
        "domains": domain_refs,
        "types": []
    });

    let content = serde_json::to_string_pretty(&root_schema).unwrap();
    let filename = output_dir.join("root").join("schema-root.json");
    let parent = filename.parent().unwrap();
    fs::create_dir_all(parent)?;
    let mut file = String::new();
    file.push_str(&content);
    file.push('\n');
    file.push_str(&format!("// Metadata generated at: {}", metadata.generated_at));
    fs::write(&filename, file.as_bytes())?;

    Ok(())
}

/// 导出所有 Schema 文件。
#[cfg(feature = "export-schema")]
pub fn export_all_schemas(root_dir: &Path, crate_version: &str) -> Result<usize, std::io::Error> {
    use crate::agent::Agent;
    use crate::chat::{ChatCompletePayload, ChatIntentPayload, ChatPayload, ProgressEvent};
    use crate::envelope::{GatewayMessage, MessageEnvelope};
    use crate::session::{MessageDTO, Session, SessionCreateRequest};
    use crate::system::{ErrorPayload, WelcomePayload};

    let output_dir = root_dir.join(SCHEMA_OUTPUT_DIR);
    fs::create_dir_all(&output_dir)?;

    let metadata = SchemaFileMetadata {
        generated_at: chrono::Utc::now().to_rfc3339(),
        source_crate: "nova-protocol".to_string(),
        source_crate_version: crate_version.to_string(),
    };

    // 清理旧域文件
    clean_old_schemas_impl(&output_dir)?;

    // 为每个域创建目录
    for (_domain, path) in DOMAIN_PATHS.iter() {
        let domain_dir = output_dir.join(path);
        fs::create_dir_all(&domain_dir)?;

        // 清理该域下的旧文件
        if domain_dir.exists() {
            for entry in fs::read_dir(&domain_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let file_name = path.file_name().unwrap().to_str().unwrap();
                    // 跳过 .gitkeep 文件
                    if file_name != ".gitkeep" {
                        fs::remove_file(&path)?;
                    }
                }
            }
        }
    }

    let mut exported_count = 0;

    // --- Gateway Domain ---
    {
        let content = generate_schema_json::<GatewayMessage>(Some("GatewayMessage"));
        let path = output_dir.join("domains/gateway/gateway-message.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// GatewayMessage - envelope payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<MessageEnvelope>(Some("MessageEnvelope"));
        let path = output_dir.join("domains/gateway/message-envelope.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// MessageEnvelope - tagged union of all message types");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    // --- System Domain ---
    {
        let content = generate_schema_json::<WelcomePayload>(Some("WelcomePayload"));
        let path = output_dir.join("domains/system/welcome-payload.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Welcome event payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<ErrorPayload>(Some("ErrorPayload"));
        let path = output_dir.join("domains/system/error-payload.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Error event payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    // --- Chat Domain ---
    {
        let content = generate_schema_json::<ChatPayload>(Some("ChatPayload"));
        let path = output_dir.join("domains/chat/chat-payload.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Chat request payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<ProgressEvent>(Some("ProgressEvent"));
        let path = output_dir.join("domains/chat/progress-event.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Progress event during chat processing");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<ChatCompletePayload>(Some("ChatCompletePayload"));
        let path = output_dir.join("domains/chat/chat-complete-payload.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Chat completion payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<ChatIntentPayload>(Some("ChatIntentPayload"));
        let path = output_dir.join("domains/chat/chat-intent-payload.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Chat intent modification payload");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    // --- Session Domain ---
    {
        let content = generate_schema_json::<SessionCreateRequest>(Some("SessionCreateRequest"));
        let path = output_dir.join("domains/session/session-create-request.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Session creation request");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<Session>(Some("Session"));
        let path = output_dir.join("domains/session/session.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Session object");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<MessageDTO>(Some("MessageDTO"));
        let path = output_dir.join("domains/session/message-dto.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Message DTO stored in session");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    {
        let content = generate_schema_json::<crate::ContentBlockDTO>(Some("ContentBlockDTO"));
        let path = output_dir.join("domains/session/content-block-dto.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Content block in a message (tagged union by type)");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    // --- Agent Domain ---
    {
        let content = generate_schema_json::<Agent>(Some("Agent"));
        let path = output_dir.join("domains/agent/agent.schema.json");
        let mut file = String::new();
        file.push_str(&content);
        file.push('\n');
        file.push_str("// Agent definition");
        fs::write(&path, file.as_bytes())?;
        exported_count += 1;
    }

    // 生成根 Schema
    generate_root_schema(&output_dir, &metadata)?;

    // 在所有域目录下创建 .gitkeep 文件（如果不存在）
    for (_domain, path) in DOMAIN_PATHS.iter() {
        let domain_dir = output_dir.join(path);
        let gitkeep = domain_dir.join(".gitkeep");
        if !gitkeep.exists() {
            fs::write(&gitkeep, "")?;
        }
    }

    Ok(exported_count)
}

/// 清理旧域文件（内部实现）。
#[allow(dead_code)]
fn clean_old_schemas_impl(output_dir: &Path) -> Result<u32, std::io::Error> {
    let mut removed = 0;
    if output_dir.exists() {
        for entry in fs::read_dir(output_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().unwrap().to_str().unwrap();
                let is_known_domain = DOMAIN_PATHS.iter().any(|(name, _)| *name == dir_name);
                match dir_name {
                    "root" => {} // always keep root
                    _other if !is_known_domain => {
                        fs::remove_dir_all(&path)?;
                        removed += 1;
                    }
                    _ => {} // known domain, keep it
                }
            }
        }
    }
    Ok(removed)
}

/// 导出域列表快照（用于 CI 校验）。
#[cfg(feature = "export-schema")]
pub fn export_domains_snapshot(root_dir: &Path) -> Result<String, std::io::Error> {
    let output_dir = root_dir.join(SCHEMA_OUTPUT_DIR);
    let mut domains = Vec::new();

    for (domain, path) in DOMAIN_PATHS {
        let domain_path = output_dir.join(path);
        if domain_path.exists() {
            let mut files = Vec::new();
            for entry in fs::read_dir(&domain_path)? {
                let entry = entry?;
                let ep = entry.path();
                if ep.is_file() {
                    let file_name = ep.file_name().unwrap().to_str().unwrap();
                    if file_name != ".gitkeep" {
                        files.push(file_name.to_string());
                    }
                }
            }
            files.sort();
            domains.push(format!("{}: [{}]", domain, files.join(", ")));
        }
    }

    let snapshot = domains.join("\n");
    // 将快照写入文件
    let snapshot_path = output_dir.join("domains_snapshot.txt");
    fs::write(&snapshot_path, &snapshot)?;

    Ok(snapshot)
}

/// 导出 Schema 注册表 JSON 文件。
#[cfg(feature = "export-schema")]
pub fn export_registry_json(root_dir: &Path) -> Result<(), std::io::Error> {
    use crate::schema::{SchemaRegistry, SchemaTypeEntry};

    let output_dir = root_dir.join(SCHEMA_OUTPUT_DIR);
    let mut registry = SchemaRegistry::new("nova-protocol", env!("CARGO_PKG_VERSION"));

    let first_phase = crate::schema::first_phase_types();
    for (domain, pattern, schema_id) in first_phase {
        let full_name = schema_id.clone();
        let domain_str = serde_json::to_value(&domain).unwrap().as_str().unwrap().to_string();
        let schema_file = format!("{}-{}.schema.json", domain_str, schema_id.to_lowercase());
        let ts_file = format!("{}-{}.ts", domain_str, schema_id.to_lowercase());

        registry.add_type(SchemaTypeEntry {
            full_name,
            schema_id,
            domain,
            pattern,
            in_envelope: true,
            schema_file,
            ts_file,
        });
    }

    for (domain, path) in DOMAIN_PATHS {
        registry.set_domain_root(domain, &format!("schemas/{}", path));
    }

    // 写入注册表 JSON
    let json = serde_json::to_string_pretty(&registry).unwrap();
    let registry_path = output_dir.join("registry.json");
    fs::write(&registry_path, format!("{}\n", json))?;

    Ok(())
}
