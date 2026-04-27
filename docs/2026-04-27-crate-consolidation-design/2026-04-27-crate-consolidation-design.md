# 2026-04-27 crate consolidation design (修订版)

## 时间
- 创建时间: 2026-04-27
- 最后更新: 2026-04-27 (修订)

## 项目现状

### 已完成的结构性变更
上一轮开发已完成以下**目录/文件层面**的搬迁，但代码引用和编译均未修复：

| 变更 | 状态 |
|------|------|
| `nova-core`、`nova-app`、`nova-conversation` 源码文件合并到 `nova-agent/src/` | ✅ 文件已搬 |
| `channel-stdio`、`channel-websocket` 源码搬到 `channel-core/src/stdio.rs`、`websocket.rs` | ✅ 文件已搬 |
| `nova-server-stdio`、`nova-server-ws` bin 文件搬到 `nova-server/src/bin/` | ✅ 文件已搬 |
| 旧 crate 目录已删除 | ✅ 已删 |
| Cargo.toml 依赖名部分更新（`nova-gateway-core`、`nova-cli`） | ⚠️ 仅 Cargo.toml 改了，源码未改 |

### 当前遗留问题（workspace 无法编译）

1. **nova-agent**
   - `lib.rs` 未声明 `pub mod app;` 和 `pub mod conversation;`
   - `app/mod.rs` 与 `app/lib.rs` 共存冲突（`conversation/` 同理）
   - `app/lib.rs` 内部仍引用 `nova_core::`、`nova_conversation::`

2. **channel-core**
   - `stdio.rs`、`websocket.rs` 已搬入但未在 `lib.rs` 中声明模块
   - 自引用使用 `channel_core::` 而非 `crate::`
   - `Cargo.toml` 缺少 `serde_json`、`log`、`tokio-tungstenite`、`futures-util` 依赖

3. **nova-server**
   - `Cargo.toml` package name 仍为 `nova-server-ws`
   - 仅声明了 `nova-server-ws` 一个 bin，缺少 `nova_gateway_stdio`
   - 缺少 `nova-agent` 依赖
   - `lib.rs` 引用 `nova_app::`、`channel_websocket::`（已不存在）
   - 两个 bin 文件引用 `nova_core::`、`nova_app::`、`nova_server_stdio::`、`nova_server_ws::`（已不存在）

4. **nova-gateway-core**
   - `lib.rs` 引用 `nova_app::AgentApplication`（应为 `nova_agent::app::AgentApplication`）
   - `bridge.rs` 引用 `nova_app::types::*`、`nova_core::message::ContentBlock`

5. **nova-cli**
   - `main.rs` 全部使用 `nova_core::*`（应为 `nova_agent::*`）

## 整体目标
- 修复所有编译错误，使 workspace 恢复全绿。
- 完成 channel-core 到 nova-server 的合并（原 Plan 2 目标）。
- 清理 workspace 成员，达到最终目标 crate 结构。

### 最终目标 crate 结构

| Crate | 角色 |
|-------|------|
| `nova-agent` | 核心 agent 运行时 + 应用层门面 + 会话管理 |
| `nova-protocol` | JSON 协议 DTO |
| `nova-gateway-core` | 协议路由与桥接层 |
| `nova-server` | 统一 server crate（传输层 + 两个入口 bin） |
| `nova-cli` | CLI REPL |
| `deskapp/src-tauri` | Tauri 桌面应用 |

## Plan 拆分

1. **Plan 1: 完善 nova-agent 模块导出与内部引用修复**
   - 描述: 解决 mod.rs/lib.rs 冲突，声明 app 和 conversation 子模块，修复内部 `use` 路径。
   - 依赖: 无
   - 顺序: 第 1 步

2. **Plan 2: 修复下游 crate 引用 + 合并 channel-core 到 nova-server**
   - 描述: 将 channel-core 传输实现内联到 nova-server::transport，更新 nova-gateway-core / nova-cli / nova-server 中所有旧路径引用。
   - 依赖: Plan 1
   - 顺序: 第 2 步

3. **Plan 3: workspace 清理与回归验证**
   - 描述: 移除 channel-core workspace 成员，清理依赖映射，执行完整 clippy/fmt/test 验证。
   - 依赖: Plan 2
   - 顺序: 第 3 步

## 风险与待定项
- 风险 1: `app/lib.rs` 与 `app/mod.rs` 合并时可能遗漏导出项，导致下游 crate 编译失败。
- 风险 2: `channel-core::websocket` 中的 `tokio-tungstenite` 类型在搬入 nova-server 后可能与 nova-agent 的 `mcp-websocket` feature 产生版本冲突。
- 风险 3: deskapp/src-tauri 可能存在对旧 crate 名或旧 bin 名的硬编码引用。
- 待定项 1: 是否保留与现有一致的二进制名（`nova_gateway_stdio`、`nova-server-ws`）以兼容脚本和 deskapp。
