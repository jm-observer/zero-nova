# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Language

Use **Chinese** when communicating with the user. Ask for clarification when requirements are ambiguous — do not assume.

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
