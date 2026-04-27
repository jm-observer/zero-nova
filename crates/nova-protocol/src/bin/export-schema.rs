/// Schema 导出二进制入口。
///
/// 用法：
///   cargo run -p nova-protocol --bin export-schema --features export-schema
///   cargo run -p nova-protocol --bin export-schema --features export-schema -- --root <path>
///
/// 该命令会：
/// 1. 导出所有协议类型为 JSON Schema 文件
/// 2. 生成根 Schema 引用文件
/// 3. 输出域列表快照
/// 4. 生成 Schema 注册表 JSON
use nova_protocol::schema;

fn main() {
    // 检查 export-schema feature 是否启用
    let feature_enabled = cfg!(feature = "export-schema");
    if !feature_enabled {
        eprintln!("Error: 'export-schema' feature is required for this binary.");
        eprintln!("Run: cargo run -p nova-protocol --bin export-schema --features export-schema");
        std::process::exit(1);
    }

    use std::env;
    use std::path::PathBuf;

    let args: Vec<String> = env::args().collect();
    let mut root_dir = PathBuf::from(".");

    // 解析 --root 参数
    for i in 0..args.len() {
        if args[i] == "--root" && i + 1 < args.len() {
            root_dir = PathBuf::from(&args[i + 1]);
        }
    }

    println!("Schema Export (Plan 2)");
    println!("======================");
    println!("Root directory: {}", root_dir.display());
    println!();

    // 导出所有 Schema
    match crate::schema::generate::export_all_schemas(&root_dir, env!("CARGO_PKG_VERSION")) {
        Ok(count) => {
            println!("✓ Exported {} schema files", count);
        }
        Err(e) => {
            eprintln!("✗ Schema export failed: {}", e);
            std::process::exit(1);
        }
    }

    // 导出域列表快照
    match crate::schema::generate::export_domains_snapshot(&root_dir) {
        Ok(snapshot) => {
            println!();
            println!("Domain Snapshot:");
            println!("{}", snapshot);
        }
        Err(e) => {
            eprintln!("Warning: Snapshot export failed: {}", e);
        }
    }

    // 导出注册表 JSON
    match crate::schema::generate::export_registry_json(&root_dir) {
        Ok(()) => {
            println!();
            println!("✓ Schema registry exported to schemas/registry.json");
        }
        Err(e) => {
            eprintln!("Warning: Registry export failed: {}", e);
        }
    }

    println!();
    println!("Schema files are in: {}/", schema::SCHEMA_OUTPUT_DIR);
}
