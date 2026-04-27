# Plan 2: 修复下游 crate 引用 + 合并 channel-core 到 nova-server

## Plan 编号与标题
- Plan 2: 修复下游 crate 引用 + 合并 channel-core 到 nova-server

## 前置依赖
- Plan 1（nova-agent 模块导出就绪）

## 本次目标
- 将 channel-core 的 trait 定义和传输实现（stdio/ws）合并到 `nova-server::transport` 模块。
- 修复 nova-gateway-core 中所有 `nova_app::`、`nova_core::` 旧引用。
- 修复 nova-cli 中所有 `nova_core::` 旧引用。
- 修复 nova-server 的 Cargo.toml、lib.rs 和两个 bin 文件。
- 确保 `cargo check --workspace` 通过。

## 涉及文件

### nova-server 结构重建
- **修改**: `crates/nova-server/Cargo.toml`（package name、依赖、bin 声明）
- **修改**: `crates/nova-server/src/lib.rs`（声明 transport 模块，暴露 `run_stdio` / `run_server`）
- **新增**: `crates/nova-server/src/transport/mod.rs`
- **新增**: `crates/nova-server/src/transport/core.rs`（从 channel-core/src/lib.rs 搬入）
- **新增**: `crates/nova-server/src/transport/stdio.rs`（从 channel-core/src/stdio.rs 搬入）
- **新增**: `crates/nova-server/src/transport/ws.rs`（从 channel-core/src/websocket.rs 搬入）
- **修改**: `crates/nova-server/src/bin/nova_gateway_stdio.rs`（修复引用路径）
- **修改**: `crates/nova-server/src/bin/nova_gateway_ws.rs`（修复引用路径）

### nova-gateway-core 引用修复
- **修改**: `crates/nova-gateway-core/Cargo.toml`（`channel-core` → `nova-server`）
- **修改**: `crates/nova-gateway-core/src/lib.rs`（修复 import 路径）
- **修改**: `crates/nova-gateway-core/src/bridge.rs`（修复 import 路径）
- **修改**: `crates/nova-gateway-core/src/handlers/*.rs`（如有 `nova_app::` 引用）

### nova-cli 引用修复
- **修改**: `crates/nova-cli/src/main.rs`（`nova_core::` → `nova_agent::`）

### deskapp 检查
- **可能修改**: `deskapp/src-tauri/`（检查是否有旧 crate 名引用）

## 详细设计

### 1. nova-server transport 模块

将 channel-core 的三个源文件搬入 `nova-server/src/transport/`:

**transport/mod.rs**:
```rust
pub mod core;
pub mod stdio;
pub mod ws;

pub use self::core::{ChannelHandler, PeerId, ResponseSink};
```

**transport/core.rs**: 原 `channel-core/src/lib.rs` 内容，无需修改（trait 定义自包含）。

**transport/stdio.rs**: 原 `channel-core/src/stdio.rs`，修复:
- `use channel_core::{ChannelHandler, ResponseSink}` → `use super::core::{ChannelHandler, ResponseSink}`

**transport/ws.rs**: 原 `channel-core/src/websocket.rs`，修复:
- `pub use channel_core::{ChannelHandler, PeerId, ResponseSink}` → `use super::core::{ChannelHandler, PeerId, ResponseSink}`

### 2. nova-server Cargo.toml 修复

```toml
[package]
name = "nova-server"
version = "0.1.0"
edition = "2021"
description = "Unified server for zero-nova (stdio + WebSocket)"

[dependencies]
nova-agent = { workspace = true }
nova-protocol = { workspace = true }
nova-gateway-core = { workspace = true }
tokio = { workspace = true }
tokio-tungstenite = { workspace = true }
tokio-util = { workspace = true }
futures-util = { workspace = true }
log = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = { workspace = true }
chrono = { workspace = true }
clap = { workspace = true, optional = true }
sysinfo = { workspace = true, optional = true }
custom-utils = { workspace = true }

[features]
default = ["cli"]
cli = ["clap", "sysinfo"]
prod = []

[[bin]]
name = "nova_gateway_stdio"
path = "src/bin/nova_gateway_stdio.rs"

[[bin]]
name = "nova-server-ws"
path = "src/bin/nova_gateway_ws.rs"
```

注意: 新增 `nova-agent` 依赖（bin 文件需要 bootstrap）；新增 `tokio-tungstenite`、`futures-util`、`serde`（transport::ws 需要）；声明两个 bin。

### 3. nova-server lib.rs 重写

```rust
pub mod transport;

use anyhow::Result;
use nova_agent::app::AgentApplication;
use nova_gateway_core::GatewayHandler;
use std::sync::Arc;

pub async fn run_server(addr: &str, app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    transport::ws::run_server(addr, handler).await
}

pub async fn run_stdio(app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    transport::stdio::run_stdio(handler).await
}
```

### 4. nova-server bin 文件修复

两个 bin 文件的引用替换:

| 旧引用 | 新引用 |
|--------|--------|
| `nova_app::bootstrap::build_application` | `nova_agent::app::build_application` |
| `nova_core::config::OriginAppConfig` | `nova_agent::config::OriginAppConfig` |
| `nova_core::config::AppConfig` | `nova_agent::config::AppConfig` |
| `nova_core::provider::openai_compat::OpenAiCompatClient` | `nova_agent::provider::openai_compat::OpenAiCompatClient` |
| `nova_server_stdio::run_stdio(app)` | `nova_server::run_stdio(app)` |
| `nova_server_ws::run_server(&addr, app)` | `nova_server::run_server(&addr, app)` |

### 5. nova-gateway-core 引用修复

**Cargo.toml**: 将 `channel-core` 依赖替换为 `nova-server`。

**lib.rs** 替换:
| 旧引用 | 新引用 |
|--------|--------|
| `channel_core::{ChannelHandler, PeerId, ResponseSink}` | `nova_server::transport::{ChannelHandler, PeerId, ResponseSink}` |
| `nova_app::AgentApplication` | `nova_agent::app::AgentApplication` |

**bridge.rs** 替换:
| 旧引用 | 新引用 |
|--------|--------|
| `nova_app::types::{AppAgent, AppEvent, AppMessage, AppSession}` | `nova_agent::app::{AppAgent, AppEvent, AppMessage, AppSession}` |
| `nova_core::message::ContentBlock` | `nova_agent::message::ContentBlock` |

**handlers/*.rs**: 检查并替换所有 `nova_app::` 引用为 `nova_agent::app::`。

### 6. nova-cli 引用修复

`main.rs` 全量替换 `nova_core::` → `nova_agent::`:

| 旧引用 | 新引用 |
|--------|--------|
| `nova_core::agent::{AgentConfig, AgentRuntime}` | `nova_agent::agent::{AgentConfig, AgentRuntime}` |
| `nova_core::event::AgentEvent` | `nova_agent::event::AgentEvent` |
| `nova_core::mcp::client::McpClient` | `nova_agent::mcp::McpClient` |
| `nova_core::message::*` | `nova_agent::message::*` |
| `nova_core::prompt::*` | `nova_agent::prompt::*` |
| `nova_core::provider::*` | `nova_agent::provider::*` |
| `nova_core::tool::*` | `nova_agent::tool::*` |
| `nova_core::skill::*` | `nova_agent::skill::*` |
| `nova_core::config::*` | `nova_agent::config::*` |

### 7. 循环依赖风险

注意: `nova-server` 依赖 `nova-gateway-core`，而 `nova-gateway-core` 现在需要引用 `nova-server::transport` 中的 `ChannelHandler` trait。这会形成循环依赖。

**解决方案**: `ChannelHandler` trait 保留在 `nova-gateway-core` 中，而非放在 `nova-server`。具体做法:
- `transport::core`（trait 定义）保留在 `nova-gateway-core` 内部（或直接保留 channel-core 作为纯 trait crate）
- `transport::stdio` 和 `transport::ws`（传输实现）放在 `nova-server`
- `nova-server` 依赖 `nova-gateway-core`（获取 trait + handler）

**备选方案**: 保留 `channel-core` 作为最小 trait crate（仅 `ChannelHandler` + `ResponseSink`），nova-gateway-core 和 nova-server 都依赖它。这是当前结构的自然延续，改动最小。

**推荐**: 采用备选方案——保留 channel-core 作为 trait crate，将 stdio.rs 和 websocket.rs 搬入 nova-server::transport。这样避免循环依赖且改动最小。

## 测试案例

- 正常路径:
  - `cargo check --workspace` 零错误。
  - `cargo build -p nova-server --bin nova_gateway_stdio` 编译成功。
  - `cargo build -p nova-server --bin nova-server-ws` 编译成功。
  - `cargo build -p nova-cli` 编译成功。
- 边界条件:
  - `nova-gateway-core` 的 `ChannelHandler` impl 正确关联到 `GatewayHandler`。
  - nova-server lib 的 `run_server` / `run_stdio` 可被 bin 文件正确调用。
- 异常场景:
  - 无循环依赖报错。
  - 无 feature 冲突。
