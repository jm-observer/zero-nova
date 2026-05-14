# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 语言

- 与用户沟通使用中文。
- 需求不明确时先澄清，不要自行假设。

## 设计与计划文档（必需）

进行功能计划、方案设计或架构调整时，必须先在 `docs/` 下编写文档，再实施代码变更。

### 文档结构

每次设计任务由 1 个总览文档 + 多个 Plan 子文档组成：

```text
docs/
├── <YYYY-MM-DD>-<topic>.md
├── <YYYY-MM-DD>-<topic>-plan-1.md
├── <YYYY-MM-DD>-<topic>-plan-2.md
└── ...
```

### 总览文档应包含

- 时间（创建/更新）
- 项目现状
- 整体目标
- Plan 拆分（依赖关系与执行顺序）
- 风险与待定项（可选）

### 每个 Plan 子文档应包含

- Plan 编号与标题
- 前置依赖
- 本次目标（可验证）
- 涉及文件
- 详细设计（模块、接口、数据流、关键实现思路）
- 测试案例（正常/边界/异常）

## 常用开发命令

### Rust 工作区

```bash
# 构建
cargo build --workspace --release

# 全量检查循环（任务完成前必须全部通过）
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace

# 运行 CLI REPL
cargo run -p nova-cli --bin nova_cli -- chat

# 运行一次性 prompt
cargo run -p nova-cli --bin nova_cli -- run "your prompt"

# 启动 Gateway（WebSocket）
cargo run -p nova-server-ws --bin nova-server-ws

# 启动 Gateway（stdio）
cargo run -p nova-server-ws --bin nova_gateway_stdio
```

### 单元测试 / 定位测试

```bash
# 运行某个 crate 的全部测试
cargo test -p nova-agent

# 运行单个测试（按测试名过滤）
cargo test -p nova-agent test_parse_skills_command

# 运行某个测试文件的相关测试（名称过滤）
cargo test -p nova-cli cli
```

### 桌面端（Tauri + 前端）

```bash
# 在 deskapp/ 目录执行
cd deskapp

# 前端开发
pnpm dev

# 完整桌面开发（含 tauri shell）
pnpm tauri dev

# 前端单测
pnpm test

# 运行单个前端测试文件
pnpm test -- src/__tests__/chat-service.test.ts

# E2E
pnpm test:e2e

# 桌面构建
pnpm tauri build
```

## 架构速览（Big Picture）

Zero-Nova 是三层结构的 Agent Runtime：

1. **Gateway Sidecar（Rust）**
   - 对外提供 stdio / WebSocket 通道，处理协议路由、会话事件、工具执行编排。
   - 入口：`crates/nova-server/src/bin/nova_gateway_stdio.rs`、`crates/nova-server/src/bin/nova_gateway_ws.rs`。
   - 路由与分发核心：`nova-gateway-core`（`GatewayHandler` + router/bridge/push center）。

2. **Agent Core（Rust）**
   - 核心能力集中在 `nova-agent`：
     - turn 执行与 runtime（agent）
     - prompt 组装与裁剪（prompt）
     - tool registry 与内置工具（tool）
     - skill 注册与策略（skill）
     - 会话与持久化（conversation，SQLite）
     - provider 适配（provider）
   - `nova-protocol` 定义 Gateway 的消息 DTO 与 envelope，是前后端协议契约。

3. **桌面壳 + 前端（Tauri + TS/Vite）**
   - `deskapp/src-tauri`：负责 sidecar 生命周期管理、系统能力暴露（tauri commands）、托盘与窗口控制。
   - `deskapp/src`：通过 `GatewayClient` 走 WebSocket 协议收发消息，组织 UI 状态、会话与进度事件渲染。

## 关键 crate 职责

- `nova-agent`：Agent 主循环、工具系统、技能系统、会话与状态。
- `nova-protocol`：前后端共享协议类型（消息 envelope / payload）。
- `nova-gateway-core`：把通道层请求分发到 AgentApplication，并做事件桥接。
- `nova-server-ws`：统一 server 入口（stdio + ws 二进制）。
- `channel-core`：通道抽象与 stdio/ws 传输适配。
- `nova-cli`：本地 REPL/one-shot 调试入口。
- `deskapp/src-tauri`：桌面容器，管理 sidecar 生命周期。

## 配置与运行时关系

- 工作区配置文件：`.nova/config.toml`。
- `nova-cli` 与 `nova-server-ws` 都通过 `resolve_workspace + AppConfig::load_from_file` 读取配置。
- 桌面端通过 tauri command `get_gateway_config` 获取 sidecar 网关地址，再由前端 `GatewayClient` 建连。

## 代码约束（来自现有规范）

- 错误处理：lib 代码使用 `anyhow::Result` + `?`；`unwrap/expect` 仅限 `main.rs` 和测试。
- 异步：基于 tokio；禁止在 async 上下文调用阻塞 I/O（使用 tokio API 或 `spawn_blocking`）。
- HTTP：`reqwest` 必须禁用默认特性并使用 rustls（禁止 OpenSSL）。
- 日志：使用 `log` 宏；应用日志禁止 `println!`。
- 依赖：未经用户明确同意不得新增依赖；workspace 成员通过 `{ workspace = true }` 引用。
- `unsafe`：默认禁止；确需使用必须写清安全性证明。

## CI / 发布

- 目标平台：`x86_64-pc-windows-msvc`、`aarch64-unknown-linux-gnu`。
- 推送 `v*` tag 会触发发布流程。
- 推送前必须本地完成 clippy + fmt + test 全通过。