# Plan 1: 修复 WebSocket 消息预览 Panic

- **前置依赖**: 无
- **本次目标**: 修改 `crates/channel-core/src/websocket.rs` 中的日志预览逻辑，使用更健壮的字符级截断方式。

## 涉及文件
- `crates/channel-core/src/websocket.rs`

## 详细设计
当前代码：
```rust
let max_len = 500usize;
let slice_len = json_str
    .char_indices()
    .find(|&(byte_idx, _)| byte_idx >= max_len)
    .map(|(byte_idx, char_)| byte_idx + char_.len_utf8())
    .unwrap_or(json_str.len());
let preview = if json_str.len() > max_len {
    format!("{}... ({} bytes)", &json_str[..slice_len], json_str.len())
} else {
    json_str.to_string()
};
```

修改后的设计：
直接利用 Rust 的 `chars()` 迭代器进行安全截断。虽然这种方式涉及 O(N) 的迭代，但在日志预览场景（且截断长度较小，如 500 字符）下性能损失微乎其微，且能保证绝对的字符边界安全。

```rust
let max_chars = 500usize;
let preview = if json_str.chars().nth(max_chars).is_some() {
    let truncated: String = json_str.chars().take(max_chars).collect();
    format!("{}... ({} bytes)", truncated, json_str.len())
} else {
    json_str.to_string()
};
```

## 测试案例
1. **正常 JSON**: 验证长度小于 500 的 JSON 正常记录。
2. **长 JSON**: 验证长度大于 500 的 JSON 被正确截断且不崩溃。
3. **多字节字符边界**: 在第 500 个字符处使用 Emoji 或中文字符，验证不再发生 Panic。
