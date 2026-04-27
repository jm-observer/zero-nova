# Plan 3: workspace 清理与回归验证

## Plan 编号与标题
- Plan 3: workspace 清理与回归验证

## 前置依赖
- Plan 2

## 本次目标
- 清理 workspace 成员列表和依赖映射，移除已合并 crate 的残留声明。
- 执行完整 clippy/fmt/test 验证循环，确保全绿。
- 检查 deskapp 相关引用是否需要同步更新。
- 更新 CLAUDE.md 中的架构说明。

## 涉及文件
- **修改**: `Cargo.toml`（root，清理 workspace members 和 dependencies）
- **修改**: `CLAUDE.md`（更新 Crate Responsibilities 表格）
- **可能修改**: `deskapp/src-tauri/Cargo.toml`（如有旧 crate 引用）
- **可能修改**: `deskapp/src-tauri/src/*.rs`（如有旧 crate 名硬编码）
- **删除**: `crates/channel-core/src/stdio.rs`（代码已搬入 nova-server）
- **删除**: `crates/channel-core/src/websocket.rs`（代码已搬入 nova-server）

## 详细设计

### 1. workspace members 清理

根据 Plan 2 的推荐方案（保留 channel-core 作为 trait crate），最终 members:

```toml
[workspace]
members = [
  "crates/nova-agent",
  "crates/nova-protocol",
  "crates/nova-gateway-core",
  "crates/nova-server",
  "crates/nova-cli",
  "crates/channel-core",
  "deskapp/src-tauri",
]
```

如果 Plan 2 最终选择将 channel-core trait 也合并到 nova-gateway-core，则移除 `crates/channel-core`。

### 2. workspace.dependencies 清理

确认 internal crate 依赖映射完整且无悬空:

```toml
# Internal crates
nova-agent = { path = "crates/nova-agent" }
nova-protocol = { path = "crates/nova-protocol" }
nova-gateway-core = { path = "crates/nova-gateway-core" }
nova-server = { path = "crates/nova-server" }
channel-core = { path = "crates/channel-core" }
```

移除已不存在的 crate path（如 nova-core、nova-app、nova-conversation、channel-stdio、channel-websocket、nova-server-stdio、nova-server-ws）。

### 3. channel-core 瘦身

删除已搬入 nova-server 的文件:
- `crates/channel-core/src/stdio.rs`
- `crates/channel-core/src/websocket.rs`

确保 channel-core 仅保留 `lib.rs`（trait + ResponseSink 定义）。

同时从 channel-core 的 Cargo.toml 中移除仅被 stdio/ws 使用的依赖（如 `serde_json`、`log`、`tokio-tungstenite`、`futures-util`——目前这些依赖实际上未在 Cargo.toml 中声明，所以此步可能为空操作）。

### 4. deskapp 检查

检查 `deskapp/src-tauri/` 中:
- `Cargo.toml` 是否依赖旧 crate（nova-core、nova-app 等）
- `tauri.conf.json` 中 sidecar 二进制名是否匹配 `nova_gateway_stdio` / `nova-server-ws`
- Rust 源码中是否有旧 crate 名的 import

### 5. CLAUDE.md 更新

更新 Architecture 和 Crate Responsibilities 部分，反映合并后的结构。

### 6. 强制验证循环

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```

三步全部通过后本次整合完成。

### 7. 回滚策略

- 整个整合在 git 中作为一系列小 commit，每个 Plan 完成后提交一次。
- 如需回滚，使用 `git revert` 按 Plan 倒序逐步回退。
- 最坏情况下可通过 `git reset --hard 765bf63`（当前 HEAD）恢复到整合前状态。

## 测试案例

- 正常路径:
  - `cargo clippy --workspace -- -D warnings` 无 warning。
  - `cargo fmt --check --all` 无格式问题。
  - `cargo test --workspace` 所有测试通过。
  - `cargo build -p nova-server --release` 编译成功。
  - `cargo run -p nova-server --bin nova-server-ws -- --help` 输出帮助信息。
  - `cargo run -p nova-cli --bin nova_cli -- --help` 输出帮助信息。
- 边界条件:
  - workspace 中无悬空依赖（`cargo check --workspace` 无 "can't find crate" 错误）。
  - deskapp 如使用 sidecar，二进制名匹配。
- 异常场景:
  - 若 deskapp 存在旧引用，需同步修复后再验证。
