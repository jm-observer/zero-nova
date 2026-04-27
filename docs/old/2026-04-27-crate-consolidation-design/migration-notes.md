# Crate Consolidation Migration Notes

- 日期: 2026-04-27
- 关联 Plan: Plan 3（workspace 成员与依赖清理）、Plan 4（回归验证与回滚）

## 迁移结果

- workspace 成员已移除：`channel-core`、`channel-stdio`、`channel-websocket`。
- `nova-server` 与 `nova-gateway-core` 不再依赖 `channel-core`。
- `ChannelHandler`/`ResponseSink`/`PeerId` 已迁移到 `nova-gateway-core::transport`。
- `nova_server::transport::core` 保留兼容导出路径：
  - `nova_server::transport::core::ChannelHandler`
  - `nova_server::transport::core::ResponseSink`
  - `nova_server::transport::core::PeerId`

## 命令映射（旧 -> 新）

- `cargo run -p nova-server-ws --bin nova-server-ws` -> `cargo run -p nova-server --bin nova-server-ws`
- `cargo run -p nova-server-stdio --bin nova_gateway_stdio` -> `cargo run -p nova-server --bin nova_gateway_stdio`
- `cargo run --bin nova-server-ws`（在多包 workspace 下）-> `cargo run -p nova-server --bin nova-server-ws`

## 回滚策略

- 回滚点 A（成员调整失败）:
  - 恢复根 `Cargo.toml` 的 `members` 和 `[workspace.dependencies]` 中 `channel-*` 条目。
- 回滚点 B（类型迁移失败）:
  - 恢复 `nova-gateway-core` 对 `channel-core` 的依赖和 `use channel_core::*` 引用。
- 回滚点 C（运行命令兼容性问题）:
  - 保留 `-p nova-server --bin ...` 新命令，同时在文档中补充兼容说明，避免脚本硬编码旧 package 名。

## 发布前检查顺序

1. `cargo clippy --workspace -- -D warnings`
2. `cargo fmt --check --all`
3. `cargo test --workspace`
4. 按目标平台构建:
   - `cargo build --workspace --release --target x86_64-pc-windows-msvc --features prod`
   - `cargo build --workspace --release --target aarch64-unknown-linux-gnu --features prod`