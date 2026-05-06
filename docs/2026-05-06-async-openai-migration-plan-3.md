# Plan 3: 调用点适配与清理

## Plan 编号与标题

Plan 3: 调用点适配与清理

## 前置依赖

Plan 2（OpenAiCompatClient 替换实现）

## 本次目标

1. 更新所有 `OpenAiCompatClient::new` 调用点，确保 base_url 格式兼容
2. 删除不再需要的 `openai_compat/types.rs`（旧的自定义流式类型）
3. 确认 `sse.rs` 仅被 Anthropic client 引用，不误删
4. 运行完整检查周期（clippy + fmt + test），确保全部通过

## 涉及文件

| 文件 | 操作 |
|------|------|
| `crates/nova-server/src/bin/nova_gateway_ws.rs:71` | 适配构造调用 |
| `crates/nova-server/src/bin/nova_gateway_stdio.rs:54` | 适配构造调用 |
| `crates/nova-cli/src/main.rs:156` | 适配构造调用 |
| `crates/nova-agent/src/tool/builtin/agent.rs:106` | 适配构造调用 |
| `crates/nova-agent/src/provider/openai_compat/types.rs` | **删除** |
| `crates/nova-agent/src/provider/openai_compat.rs` | 移除 `pub mod types;` 声明 |

## 详细设计

### 1. 调用点盘点

当前所有 `OpenAiCompatClient::new(api_key, base_url)` 调用：

| 位置 | 代码 |
|------|------|
| `nova_gateway_ws.rs:71` | `OpenAiCompatClient::new(config.provider.api_key.clone(), config.provider.base_url.clone())` |
| `nova_gateway_stdio.rs:54` | 同上 |
| `nova_cli/main.rs:156` | 同上 |
| `agent.rs (builtin/agent.rs):106` | 同上 |

新实现的 `OpenAiCompatClient::new` 签名保持 `fn new(api_key: String, base_url: String) -> Self` 不变，因此这些调用点**无需修改签名**。

### 2. base_url 格式兼容

关键问题：async-openai `with_api_base` 的行为。

当前 base_url 的典型值：
- `https://api.openai.com/v1`
- `https://custom-proxy.example.com/v1`

async-openai 内部拼接路径为 `{api_base}/chat/completions`。当前手写代码同样拼接 `{base_url}/chat/completions`。两者行为一致，无需额外处理。

但需在 `OpenAiCompatClient::new` 内部做防御性处理：

```rust
pub fn new(api_key: String, base_url: String) -> Self {
    let base = base_url.trim_end_matches('/').to_string();
    let config = OpenAIConfig::new()
        .with_api_key(api_key)
        .with_api_base(base);
    Self {
        client: OpenAiSdkClient::with_config(config),
    }
}
```

### 3. 删除旧类型文件

`crates/nova-agent/src/provider/openai_compat/types.rs` 包含：
- `ChatCompletionChunk`
- `ChunkChoice`
- `ChunkDelta`
- `ChunkToolCall`
- `ChunkFunction`
- `OpenAiUsage`
- `OpenAiPromptTokensDetails`

这些类型在 Plan 2 完成后不再被引用。确认删除。

同时从 `openai_compat.rs` 中移除：
```rust
// 删除这行
pub mod types;
// 删除这行
use crate::provider::openai_compat::types::ChatCompletionChunk;
```

### 4. 确认 sse.rs 保留

`sse.rs` 的引用点：

| 文件 | 引用方式 |
|------|---------|
| `provider/anthropic.rs` | `use crate::provider::sse::SseParser;` |
| `provider/openai_compat.rs`（旧） | `use crate::provider::sse::{RawSseEvent, SseParser};` |

Plan 2 完成后，`openai_compat.rs` 不再引用 `sse.rs`。`anthropic.rs` 仍然依赖它。因此 `sse.rs` 保留。

### 5. 依赖清理检查

Plan 2 后，`openai_compat.rs` 不再直接使用以下 crate：
- `reqwest`（HTTP 由 async-openai 内部管理）

但 `reqwest` 仍被以下模块使用：
- `provider/anthropic.rs`
- `provider/health.rs`
- `tool/builtin/web_fetch.rs`（可能）
- 其他工具

因此 `reqwest` 依赖保留在 `nova-agent/Cargo.toml` 中，不删除。

### 6. 完整检查周期

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```

重点关注：
- `unused import` 警告（移除旧 import）
- `dead_code` 警告（删除旧类型后）
- 类型不匹配（u32 vs u64 转换）
- async-openai 与 workspace reqwest 版本是否冲突

## 测试案例

### 1. 编译通过

所有 crate（nova-agent、nova-server、nova-cli、deskapp/src-tauri）编译无错误、无警告。

### 2. Clippy 无警告

`cargo clippy --workspace -- -D warnings` 零输出。

### 3. 已有测试通过

`cargo test --workspace` 所有既有测试通过，包括：
- `provider/health.rs` 中的 `infer_provider_kind` 和 `build_probe_url` 测试
- 其他模块的单元测试

### 4. 集成验证（手动）

使用 `cargo run --bin nova_cli -- chat` 启动 CLI，发送一条消息：
- 验证流式响应正常输出
- 验证 token usage 在 session 中正确记录
- 验证工具调用（如 read_file）正常工作

### 5. 多 Provider 兼容验证（手动）

配置不同的 base_url 测试：
- OpenAI 官方 API
- 自建代理 / 第三方兼容 API（如 OpenRouter、Together AI 等）
- 验证 base_url 拼接正确，请求能正常发送和接收
