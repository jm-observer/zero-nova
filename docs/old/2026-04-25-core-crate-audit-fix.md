# Core Crate Audit & Fix

**Date**: 2026-04-25

## 1. Current State

Five core crates were audited for concurrency correctness, performance, and adherence to code standards:

| Crate | Lines | Primary Concerns |
|-------|-------|------------------|
| `nova-core` | ~2200 | Blocking mutex in async, double-clone in `run_turn`, agent config drift |
| `nova-conversation` | ~500 | `std::sync::RwLock` in async context, read-through cache race |
| `nova-app` | ~350 | Over-generification of `AgentApplicationImpl` |
| `nova-protocol` | ~200 | `Unknown` envelope drop type info |
| `nova-cli` | ~320 | Hard-coded config values, incomplete debug commands |

All three CI checks pass: `clippy -- -D warnings`, `cargo fmt --check`, `cargo test --workspace`.

## 2. Goals

1. **Eliminate blocking locks in async paths** — Replace `std::sync::Mutex` / `std::sync::RwLock` with `tokio` equivalents where they block async threads.
2. **Eliminate double-clone in agent turn preparation** — Use `Arc::try_unwrap` properly.
3. **Fix read-through cache race** — Ensure read-through `SessionService::get` de-duplicates concurrent cold loads.
4. **Synchronize CLI vs app config defaults** — Use `AppConfig` values in CLI.
5. **Add mutex poisoning comments** — Document why poisoning recovery is safe per AGENTS.md.
6. **Improve `ToolRegistry` locking** — Refactor to async-safe `tokio::sync::Mutex`.

## 3. Detailed Design

### 3.1 `ToolRegistry`: `std::sync::Mutex` → `tokio::sync::Mutex`

**Files**: `nova-core/src/tool.rs`

**Problem**: `ToolRegistry` uses `std::sync::Mutex` for `tools` and `deferred` fields. In async context, holding this lock while waiting on I/O would block the tokio thread. Currently all lock operations are synchronous and inline, so blocking is brief, but the pattern is not future-proof.

**Solution**: Keep `std::sync::Mutex` but move blocking lock into `spawn_blocking`. The lock hold time is short (O(1) vector push/pop), so `spawn_blocking` is not strictly necessary yet. A cleaner approach: wrap the lock methods in `spawn_blocking` to follow the pattern that blocking = off-main-thread.

**Verification**: Run `cargo test --workspace` — all tool tests pass.

### 3.2 `Session`: Blocking `RwLock` → non-blocking pattern

**Files**: `nova-conversation/src/session.rs`, `nova-conversation/src/service.rs`

**Problem**: `Session.history` and `Session.control` use `std::sync::RwLock`. In async `append_message`, `get_history`, `list_sorted`, the lock blocks the tokio thread while held.

**Solution**: Keep `std::sync::RwLock` but ensure:
1. Lock hold time is minimal in async context
2. Add `.lock().or_else(|p| p.into_inner())` patterns where appropriate
3. Document why replacement with `tokio::sync::RwLock` is deferred (RwLock in conversation is primarily for in-process access, not cross-process).

**Verification**: Run `cargo test --workspace`.

### 3.3 Read-through cache race fix

**Files**: `nova-conversation/src/service.rs`

**Problem**: Concurrent `get()` calls for same missing session can create duplicate `Arc<Session>` entries.

**Solution**: Add a `FnOnce`-based registration mechanism. When a session is being loaded, register an async join handle. Subsequent calls for the same session join that handle. This is a lightweight `tokio::sync::Notify` + `HashMap<String, JoinHandle>`.

**Verification**: Add unit test for concurrent cold-load de-duplication.

### 3.4 `run_turn` double-clone fix

**Files**: `nova-core/src/agent.rs`

**Problem**: `Arc::try_unwrap(ctx.history).unwrap_or_else(|h| (*h).clone())` — if try_unwrap fails, we clone then call `collect::<Vec<_>>()` which creates another clone.

**Solution**: Always move — if try_unwrap succeeds, consume the Arc. If it fails, clone. Either way, the result is a `Arc<Vec<Message>>` consumed by the turn loop.

### 3.5 CLI config synchronization

**Files**: `nova-cli/src/main.rs`

**Problem**: `max_iterations: 15` and `max_tokens: 4096` hardcoded.

**Solution**: Read from `config.gateway.max_iterations` and `config.gateway.max_tokens` with defaults matching the current CLI values.

## 4. Test Cases

| Test | File | Coverage |
|------|------|----------|
| `execute_supports_legacy_tool_names` | `nova-core/src/tool.rs` | Already exists |
| `tool_search_can_load_deferred_tool` | `nova-core/src/tool.rs` | Already exists |
| **New**: `concurrent_get_cold_load_dedup` | `nova-conversation/src/service.rs` | Cache race fix |
| **New**: `tool_registry_async_mutex_is送的` | `nova-core/src/tool.rs` | Mutex replacement |
| **New**: `try_unwrap_does_not_double_clone` | `nova-core/src/agent.rs` | Clone fix |
| **New**: `cli_uses_config_defaults` | `nova-cli/src/main.rs` | Config sync |

## 5. Risks / Unknowns

1. **`tokio::sync::Mutex` overhead** — ~10-20% per lock/unlock vs `std::sync::Mutex`. Acceptable for small lock-hold times.
2. **`std::sync::RwLock` in async** — After migration, ensure async callers can still hold locks. Small overhead is acceptable.
3. **Cache race fix complexity** — Adding async join handles adds memory pressure for high-traffic sessions. Monitor RSS growth.
