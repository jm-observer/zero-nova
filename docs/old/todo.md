# TODO

## 待确认事项

### 1. `trim_iteration_messages_if_needed` 权重比较逻辑

**文件：** `crates/zero-nova/src/agent.rs`

**问题：** 当前权重比较使用 `>=`，权重越高的消息被保留（不被裁剪）。

```rust
match best {
    Some((_, _, best_weight)) if best_weight >= weight => {}  // 不更新，保留旧的最佳
    _ => best = Some((idx, pair_tokens, weight)),
}
```

**权重定义：**
- Read 操作：`+1000`
- 旧消息：`+100`
- Token 数量：`+1`

**疑问：** 这个逻辑是否是有意为之？

- 如果权重越高表示"越应该被裁剪"，则应改为 `best_weight <= weight`
- 如果权重越高表示"越应该被保留"，则当前逻辑正确

**建议：** 明确权重语义，并在注释中说明。可能需要将权重重新定义为"裁剪优先级"（越高越容易被裁剪）。

**优先级：** 中

---

## 后续优化方向

### 1. Read 重复读取提示优化

**文件：** `crates/zero-nova/src/read.rs`

当前提示："Read 操作可能重复读取，已跳过"

**建议：** 可以添加更多信息，例如：
- 重复了多少次
- 跳过了哪些内容

**优先级：** 低

### 2. 测试覆盖补充

**文件：** `crates/zero-nova/src/agent.rs`

`trim_iteration_messages_if_needed` 目前只有 1 个单元测试。

**建议补充：**
- 边界情况：messages 数量等于 limit
- 边界情况：protect_count 大于 messages 数量
- 边界情况：不需要裁剪的情况
- 验证裁剪后的消息索引正确性

**优先级：** 中

### 3. loop_guard 超时默认值

**文件：** `crates/zero-nova/src/loop_guard.rs`

当前超时默认值为 0（无超时）。

**建议：** 考虑是否需要非零默认值，避免无限循环。

**优先级：** 低

---

## 文档更新

### 1. code-review.md 同步

**文件：** `docs/code-review.md`

当前文档可能未包含最新的代码审查结果。

**建议：** 将本次审查发现的问题和结论同步到 code-review.md。

**优先级：** 低

---
