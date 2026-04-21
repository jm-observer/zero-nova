# Protocol Design & Known Issues

## Message Envelope Structure

The system uses a unified message envelope (consistent with OpenFlux frontend):
```json
{
  "id": "string",
  "type": "string",
  "payload": { ... } // Optional for certain types
}
```

## Known Issues & Resolutions

### Issue: Missing `payload` field in certain message types
**Problem:**
When the client sends messages that do not contain a `payload` field (e.g., `{"type":"browser.status","id":"..."}`), the `GatewayMessage` parser fails with a `missing field payload` error. This is due to the `#[serde(tag = "type", content = "payload")]` configuration in `MessageEnvelope`, which strictly expects the `payload` key.

**Solution:**
For message types that may arrive without a payload, the following pattern is implemented in `src/gateway/protocol.rs`:
1. The variant payload type is changed to `Option<serde_json::Value>`.
2. The `#[serde(default)]` attribute is applied to the variant.

This allows Serde to successfully parse messages missing the `payload` key by assigning them a `None` value.

**Handling in Router:**
In `src/gateway/router.rs`, these optional payloads are handled using `.unwrap_or_default()` to provide a fallback empty JSON object, ensuring seamless integration with existing handler logic.

**Affected Files:**
- `src/gateway/protocol.rs`
- `src/gateway/router.rs`
