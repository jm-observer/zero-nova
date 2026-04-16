# Gateway Protocol Implementation Design

This document outlines the technical design for aligning the Rust implementation of the Gateway Protocol in `src/gateway/protocol.rs` with the formal specification in `docs/gateway-protocol.md`.

## 1. Objective
Refactor the `GatewayMessage` and `MessageEnvelope` structures to ensure 100% compatibility with the TypeScript specification, covering all command categories and event types while maintaining Rust's type safety.

## 2. Gap Analysis (Comprehensive)

| Feature / Category | TypeScript Specification | Current Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **Core Structure** | `id` is optional (`id?`) | `id` is mandatory (`String`) | 🔴 **Fixed needed** |
| **Field Naming** | Mostly `camelCase` (`sessionId`) | Mix of `snake_case` / `alias` | 🟡 **Needs Standardization** |
| **Empty Payloads** | `payload?` (omitted if empty) | Forced via `content = "payload"` | 🔴 **Needs Unit Variants** |
| **Agent Management**| Create, Update, Delete, Switch | Only `list` (incomplete fields) | 🔴 **Missing** |
| **Scheduler API** | List, Runs, Pause/Resume, Trigger | None | 🔴 **Missing** |
| **Memory & Distill** | Stats, List, Search, Graph, Trigger | None | 🔴 **Missing** |
| **Router & Weixin** | Config, Status, Test, QrBind/Login | Only simple `get` placeholders | 🔴 **Missing** |
| **Events (Push)** | Progress, Collaboration, QrCode | Inconsistent `ProgressType` | 🔴 **Inconsistent** |

## 3. Proposed Design

### 3.1 GatewayMessage (Foundation)
The root structure must support optional IDs to allow server-to-client events.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}
```

### 3.2 MessageEnvelope (Polymorphism)
We use the `tag = "type", content = "payload"` attribute.
- **Commands without payloads** (e.g., `sessions.list`) -> Unit Variant: `ListSessions`
- **Commands with payloads** -> Tuple/Struct Variant: `Chat(ChatPayload)`

**Crucial Change**: All Payload structs will use `#[serde(rename_all = "camelCase")]` to match the frontend, eliminating the need for manual `#[serde(alias = "...")]`.

### 3.3 Core Payload Redesign
#### ProgressEvent (Section 4.2 Alignment)
The current `ProgressType` will be flattened or restructured to match the specification:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String, // 'thinking' | 'tool_start' | ...
    pub iteration: Option<i32>,
    pub tool: Option<String>,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub thinking: Option<String>,
    pub token: Option<String>,
    pub output: Option<String>,
    pub session_id: Option<String>,
}
```

## 4. Implementation Detailed Mapping

### 4.1 Scheduler payload mapping
- `scheduler.list` -> `SchedulerList` (Unit)
- `scheduler.trigger` -> `SchedulerTrigger { taskId: String }`

### 4.2 Memory payload mapping
- `memory.stats` -> `MemoryStats` (Unit)
- `memory.search` -> `MemorySearch { query: String, limit: usize }`

### 4.3 Integration mapping
- `router.qr_bind_code` -> `RouterQrBindCode { status, qr_data, expires_in, ... }`

## 5. Implementation Steps
1.  **Refactor Main Structures**: Apply `Option<id>` and `flatten` envelope.
2.  **Apply CamelCase Convention**: Add `rename_all = "camelCase"` to all relevant structs/enums.
3.  **Implement All Variants**: Fill in `MessageEnvelope` with all types from `gateway-protocol.md`.
4.  **Define Sub-Structs**: Create the necessary payload structs for each new variant.
5.  **Test Suite**: Verify serialization/deserialization for each category.

## 6. Verification
- `cargo test` for serialization format.
- Ensure `{"type": "welcome"}` (no ID, no payload) parses successfully.
- Ensure `{"id": "1", "type": "chat", "payload": {"message": "hi"}}` parses correctly.
