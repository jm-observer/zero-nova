# Session Progress Push Center - 代码实现 Review

## 时间

- Review 日期：2026-05-08
- 基于文档：session-progress-push-center.md

## 涉及文件

| 文件 | 说明 |
|------|------|
| `crates/nova-gateway-core/src/push_center.rs` | Push Center 核心实现 |
| `crates/nova-gateway-core/src/handlers/chat.rs` | Chat Handler 集成 |
| `crates/nova-gateway-core/src/bridge.rs` | AppEvent → GatewayMessage 转换 |

---

## 问题清单

### P0 - 高优先级

#### 问题 1：`PeerId` 类型不一致

**位置：** `push_center.rs`

**描述：** `peers` 使用 `PeerId` 作为 HashMap key，但 `peer_sessions` 和 `session_peers` 使用 `String`。

```rust
// peers 使用 PeerId
peers: RwLock<HashMap<PeerId, ResponseSink<GatewayMessage>>>,

// peer_sessions 和 session_peers 使用 String
peer_sessions: RwLock<HashMap<PeerId, String>>,
session_peers: RwLock<HashMap<String, HashSet<PeerId>>>,
```

**影响：** `unregister_peer` 接收 `&str` 并调用 `self.peers.write().await.remove(peer_id)`，如果 `PeerId` 不是 `String` 的 alias 或未实现 `Borrow<str>`，可能导致无法正确删除 peer。

**建议：** 统一使用 `String` 作为 peer_id 类型，或确保 `PeerId` 实现 `Borrow<str>`。

---

#### 问题 2：`broadcast_to_session_except` 中 stale peer 清理存在重复调用

**位置：** `push_center.rs` - `broadcast_to_session_except`

**描述：** 当多个并发广播同时检测到同一个 peer 失效时，可能重复调用 `unregister_peer`。虽然 `unregister_peer` 是幂等的（`HashMap::remove` 对不存在的 key 返回 `None`），但会产生额外锁竞争开销。

**建议：** 在 `unregister_peer` 中增加日志输出，或在 `stale_peers` 去重后再清理。

---

### P1 - 中优先级

#### 问题 3：`event_forwarder` 泄漏风险

**位置：** `handlers/chat.rs` - `handle_chat`

**描述：** 当 `start_turn` 失败后，代码 await `event_forwarder`：

```rust
let turn_result = match app.start_turn(&session_id, &payload.input, event_tx).await {
    Ok(res) => res,
    Err(e) => {
        if let Err(join_error) = event_forwarder.await {
            log::error!("Failed to join app event forwarder after start_turn error: {}", join_error);
        }
        // ...
        return;
    }
};
```

但 `event_tx` 在 `start_turn` 失败时可能未被消费端关闭，导致 `event_forwarder` 中的 `while let Some(event) = event_rx.recv().await` 一直阻塞等待。

**建议：** 在错误路径中显式关闭 `event_tx` 或设置超时。

---

#### 问题 4：直连发送与广播使用不同的发送通道

**位置：** `handlers/chat.rs` - `event_forwarder`

**描述：** 当前实现中：

```rust
// 1. 直连发送给当前连接
if outbound_tx_clone.send_async(gateway_msg.clone()).await.is_err() {
    break;
}

// 2. 广播给同 session 的其他连接
push_center_clone
    .broadcast_to_session_except(&session_id_clone, Some(&peer_id_owned), gateway_msg)
    .await;
```

`outbound_tx_clone` 是 handler 接收参数时传入的 `ResponseSink`，而 `push_center` 管理的是通过 `register_peer` 注册的 peers。如果两者不是同一个连接，可能导致：
- 当前连接收到两次消息（一次直连，一次广播）
- 或当前连接只收到直连消息，其他连接只收到广播

**建议：** 在 `subscribe_peer_to_session` 时确保当前连接也被注册到 push_center，或在广播时排除逻辑与直连逻辑一致。

---

### P2 - 低优先级

#### 问题 5：缺少 `PeerId` 类型一致性验证

**位置：** `push_center.rs`

**描述：** 代码中混合使用 `PeerId` 和 `String`，但没有明确的类型转换或验证。

**建议：** 添加类型别名或 wrapper 确保一致性。

---

#### 问题 6：测试覆盖不足

**位置：** `push_center.rs` - `tests` 模块

**缺失测试：**
1. stale peer 清理测试
2. `unregister_peer` 清理 session 映射测试
3. 并发广播竞态测试
4. `broadcast_to_session_except` 的 excluded peer 测试
5. PeerId 类型一致性测试

**建议：** 补充上述测试用例。

---

#### 问题 7：`remove_peer_from_session` 中 session 清理逻辑

**位置：** `push_center.rs` - `remove_peer_from_session`

**描述：** 当 session 下最后一个 peer 被移除时，session 条目会被删除。但如果后续有新的 peer 订阅该 session，需要重新创建。

**建议：** 确认这个行为是否符合预期（目前是合理的）。

---

## 文档与实现对齐检查

| 文档目标 | 实现状态 | 备注 |
|---------|---------|------|
| 管理已建立的 WebSocket 连接 | ✅ | `register_peer` / `unregister_peer` |
| 按 Session 维度实时分发事件 | ✅ | `broadcast_to_session` + `session_peers` |
| 连接断开时自动移除失效连接 | ✅ | `broadcast_to_session_except` 中检测 `send_async` 失败 |
| 新连接建立后订阅指定 Session | ✅ | `subscribe_peer_to_session` 支持重订阅 |
| 前端刷新后拉取 Session 进度快照 | ⚠️ | 依赖外部持久化层，push_center 未直接实现 |

---

## 评分汇总

| 维度 | 评分 | 说明 |
|------|------|------|
| 架构设计 | ⭐⭐⭐⭐ | 双向映射清晰，职责单一 |
| 代码健壮性 | ⭐⭐⭐ | 缺少边界条件处理（PeerId 类型一致性） |
| 测试覆盖 | ⭐⭐⭐ | 核心场景覆盖，缺少边缘情况 |
| 文档对齐 | ⭐⭐⭐⭐ | 实现基本覆盖文档目标 |

---

## 修复建议优先级

1. **P0-1**: 统一 `PeerId` 类型使用
2. **P0-2**: 处理并发广播中的 stale peer 重复清理
3. **P1-3**: 修复 `event_forwarder` 泄漏风险
4. **P1-4**: 确认直连与广播的通道一致性
5. **P2-6**: 补充缺失的测试用例
