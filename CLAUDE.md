# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Language

Use **Chinese** when communicating with the user. Ask for clarification when requirements are ambiguous — do not assume.


## 计划与设计文档

进行功能计划、方案设计或架构调整时，**必须**在项目根目录的 `docs/` 目录下创建设计文档。

### 文档结构

每次设计任务由**一个总览文档**和**若干 Plan 子文档**组成：

```
docs/
├── <日期>-<主题>.md              # 总览文档
├── <日期>-<主题>-plan-1.md       # Plan 1
├── <日期>-<主题>-plan-2.md       # Plan 2
└── ...
```

示例：`docs/2026-04-19-metrics-export.md` + `docs/2026-04-19-metrics-export-plan-1.md`

### 总览文档

总览文档描述整体背景和拆分概要，包含以下内容：

| 章节       | 说明 |
|-----------|------|
| 时间       | 文档创建 / 最后更新日期 |
| 项目现状    | 当前相关模块或功能的状态概述 |
| 整体目标    | 本次设计要达成的最终目标 |
| Plan 拆分  | 列出所有 Plan 及其简要描述、依赖关系和执行顺序 |
| 风险与待定项 | 已知风险、待确认事项（可选） |

### Plan 子文档

将详细设计**按职责或阶段**拆分为多个 Plan，每个 Plan 一个独立文件。拆分原则：

- 每个 Plan 应聚焦**单一职责**（如：数据模型定义、核心逻辑实现、API 层适配、测试补充等）
- Plan 之间的依赖关系在总览文档中明确标注
- 每个 Plan 可独立评审和实施

每个 Plan 文件包含以下内容：

| 章节       | 说明 |
|-----------|------|
| Plan 编号与标题 | 如 `Plan 1: 数据模型定义` |
| 前置依赖    | 本 Plan 依赖的其他 Plan（无则标注"无"） |
| 本次目标    | 本 Plan 要达成的具体目标，清晰可验证 |
| 涉及文件    | 本 Plan 需要新增或修改的文件列表 |
| 详细设计    | 方案说明、模块划分、接口定义、数据流、关键实现思路等 |
| 测试案例    | 覆盖正常路径、边界条件、异常场景的测试用例设计 |

### 执行流程

1. 先编写总览文档，明确整体目标和 Plan 拆分方案
2. 按顺序编写每个 Plan 子文档
3. 所有文档完成后，提交用户评审
4. 按 Plan 顺序逐个实施，每完成一个 Plan 执行一次修复流程（见下方）

## Build & Check Commands

```bash
# Build (release)
cargo build --workspace --release

# Full check cycle (must all pass before any task is complete)
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace

# Run CLI agent
cargo run --bin nova_cli -- chat

# Run WebSocket gateway
cargo run --bin nova-server-ws

# Desktop app (in deskapp/)
pnpm dev          # frontend only
pnpm tauri dev    # full Tauri app
pnpm tauri build  # release desktop build
```

**After every code change, run the full check cycle. All three must pass. Never stop mid-cycle.**

## Architecture

Zero-Nova is an AI agent framework. The runtime has three layers:

1. **Gateway sidecar** — Rust binaries handling LLM routing, tool execution, memory (SQLite), and MCP protocol. Three binaries: `nova_cli` (REPL), `nova_gateway_stdio` (NDJSON stdio), `nova-server-ws` (WebSocket on port 18801).
2. **Tauri shell** (`deskapp/src-tauri`) — Manages the sidecar lifecycle, native APIs, and file I/O.
3. **Frontend** (`deskapp/src`) — TypeScript/Vite chat UI that communicates with the Tauri shell.

### Crate Responsibilities

| Crate | Role |
|---|---|
| `nova-core` | Core agent loop: LLM clients, tool dispatch, MCP integration |
| `nova-conversation` | Conversation state and history management |
| `nova-app` | Application-level facade; Tauri backend entry point |
| `nova-protocol` | JSON DTOs for the gateway protocol |
| `nova-gateway-core` | Gateway routing and orchestration logic |
| `nova-server-stdio` | NDJSON-over-stdio server |
| `nova-server-ws` | WebSocket server |
| `channel-core` / `channel-stdio` / `channel-websocket` | Channel trait + implementations |

Configuration lives in `.nova/config.toml` (LLM providers, gateway port, agents, voice, browser).

## Code Standards (from AGENTS.md)

**Error handling**: `anyhow::Result` + `?` everywhere in lib code. No `.unwrap()` / `.expect()` outside `main.rs` and tests. No `#[allow(...)]` to suppress warnings without a comment explaining why.

**Async**: tokio (full features). Never call blocking APIs (`std::fs`, `std::thread::sleep`, sync I/O) in async contexts — use tokio equivalents or `spawn_blocking`.

**HTTP**: `reqwest` with `default-features = false, features = ["json", "rustls-tls", ...]`. No OpenSSL.

**Logging**: `log` macros (`info!`, `error!`, etc.). `println!` is forbidden for application logs.

**Visibility**: Default private. Use `pub(crate)` or `pub` only when external access is needed.

**Clones**: Prefer borrowing (`&T`). Only clone when ownership transfer is genuinely required.

**No `unsafe`**: Prohibited unless a detailed comment justifies necessity and safety guarantees.

**Dependencies**: Do not add new dependencies without explicit user approval. All workspace members must use `{ workspace = true }` — versions are declared only in the root `[workspace.dependencies]`.

## Design Documents

For any feature, architecture change, or non-trivial plan, create a design doc in `docs/` before writing code:

```
docs/<YYYY-MM-DD>-<topic>.md
```

Required sections: date, current state, goals, detailed design, test cases, risks/unknowns.

## CI / Release

Build targets: `x86_64-pc-windows-msvc`, `aarch64-unknown-linux-gnu`. Pushing a `v*` tag triggers a release. Confirm the full check cycle passes locally before pushing tags.
