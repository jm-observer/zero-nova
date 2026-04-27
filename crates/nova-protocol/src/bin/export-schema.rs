use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nova_protocol::schema::export_repository_artifacts;

fn main() -> Result<()> {
    let root = parse_root_arg().context("解析导出根目录失败")?;
    export_repository_artifacts(&root)
}

fn parse_root_arg() -> Result<PathBuf> {
    let mut args = env::args().skip(1);
    let mut root = None;

    while let Some(arg) = args.next() {
        if arg == "--root" {
            let value = args.next().context("--root 缺少路径参数")?;
            root = Some(PathBuf::from(value));
        }
    }

    Ok(root.unwrap_or(env::current_dir().context("获取当前目录失败")?))
}
