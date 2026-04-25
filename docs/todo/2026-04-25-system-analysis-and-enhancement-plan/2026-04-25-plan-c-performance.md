# Design Doc: Plan C - Performance & Synchronization (性能优化与状态同步)

**Date**: 2026-04-25
**Status**: Draft
**Author**: Claude Code

## 1. Current State (现状)

随着 Agent 能力的增强，系统在处理高频数据流和复杂状态切换时遇到了性能瓶颈。

### 1.1 流式渲染压力 (Frontend Rendering Pressure)
目前的 `AgentRuntime` 在推送 `ThinkingDelta` 和 `TextDelta` 时采用了极细粒度的“逐字”推送模式。前端 `gateway-client.ts` 接收到消息后，会立即触发状态更新。
- **问题**: 在 LLM 输出速度极快或输出超长文本时，高频的状态变更会导致 React/Vue/Vanilla JS 的重绘频率超过屏幕刷新率（60Hz），引起 UI 明显的掉帧、卡顿甚至浏览器假死。

### 1.2 状态同步滞后 (State Sync Latency)
当前的架构中，后端状态（如：当前激活的 Agent、当前 Session 的元数据、配置变更）与前端 UI 之间的同步是“被动”的。
- **问题**: 当用户通过 CLI 或其他方式修改了配置，或者后端由于某种逻辑自动切换了 Agent 时，前端 UI 无法感知，必须等待下一次用户交互或手动刷新。这种“状态不一致”会破坏用户的沉浸感。

### 1.3 通信链路开销 (Communication Overhead)
后端在 `Gateway` 与 `Agent` 之间引入了 `mpsc` 通道进行事件转发（`event_forwarder`）。
- **问题**: 虽然这种异步设计提高了吞吐量，但在处理大量微小消息（如高频 Delta）时，频繁的 Context Switch 和 Channel 操作会引入微秒级的调度延迟。虽然在单用户场景不明显，但在资源受限的环境下可能影响实时性。

---

## 2. Goals (目标)

1.  **实现平滑的流式渲染**: 引入消息聚合与渲染节流机制，确保 UI 在任何输出速度下都保持稳定流畅。
2.  **建立主动状态同步机制**: 实现从后端到前端的“状态推送”模式，确保 UI 始终反映 Agent 的真实状态。
3.  **降低通信延迟**: 优化高频小消息的传输效率。

---

## 3. Detailed Design (实现细节)

### 3.1 前端渲染优化：消息聚合与节流 (Message Batching & Throttling)

#### 3.1.1 逻辑层：缓冲区 (Buffer)
在 `gateway-client.ts` 或 `chat-service.ts` 中引入一个临时的消息缓冲区（Buffer）。
- **策略**: 不再是“收到一条消息 $\rightarrow$ 更新一次状态”，而是“收到消息 $\rightarrow$ 放入 Buffer $\rightarrow$ 定期/定量刷新”。

#### 3.1.2 实现方案：RequestAnimationFrame (rAF) 驱动
使用 `requestAnimationFrame` 来控制 UI 的更新频率。
```typescript
// 伪代码实现
class MessageBuffer {
  private buffer: Message[] = [];
  private lastUpdate = 0;
  private frameRate = 1000 / 60; // 目标 60 FPS

  add(msg: Message) {
    this.buffer.push(msg);
    this.scheduleUpdate();
  }

  private scheduleUpdate() {
    if (this.isUpdating) return;
    this.isUpdating = true;

    requestAnimationFrame((timestamp) => {
      this.flush(timestamp);
      this.isUpdating = false;
    });
  }

  private flush(timestamp: number) {
    // 只有当时间间隔足够长时才进行重大的状态合并和触发更新
    if (timestamp - this.lastUpdate >= this.frameRate) {
      const batch = [...this.buffer];
      this.buffer = [];
      this.applyBatchToState(batch); // 批量更新 State，触发单次重绘
      this.lastUpdate = timestamp;
    }
  }
}
```

### 3.2 后端状态同步：主动推送机制 (Active State Push)

#### 3.2.1 扩展协议
在 `nova-protocol` 中增加 `SessionStateUpdate` 消息类型。
```rust
pub enum MessageEnvelope {
    // ...
    // 当 Agent 状态、配置或上下文发生变化时推送
    StateUpdate(StateUpdatePayload),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateUpdatePayload {
    pub target: StateTarget, // e.g., Agent, Config, Session
    pub data: serde_json::Value,
}
```

#### 3.2.2 后端监听器 (Observer Pattern)
在 `nova-gateway-core` 中实现一个观察者模式。
- **实现**: `AgentRuntime` 或 `ConfigManager` 在关键状态变更（如 `switch_agent`）后，通过 `event_forwarder` 向所有连接的客户端广播 `StateUpdate`。

#### 3.2.3 前端响应
前端 `gateway-client.ts` 接收到 `StateUpdate` 后，通过 `event-bus` 通知对应的 UI 组件进行局部更新。

### 3.3 链路优化 (Latency Optimization)

- **消息合并策略**: 在 `event_forwarder` 层，对于连续出现的同类型高频消息（如 `ThinkingDelta`），可以在发送前进行微小的合并（例如 10ms 内的消息合并为一个包），以减少网络包的数量和解析次数。

---

## 4. Test Plan (测试计划)

### 4.1 性能压力测试 (Performance Stress Test)
- **模拟高频输出**: 编写一个脚本，模拟后端以极高的速率（例如 1ms 一个字符）发送 `TextDelta`。
- **观测指标**:
    - 测量前端 CPU 使用率。
    - 检查 UI 是否出现明显的视觉跳变或卡顿（使用 Chrome DevTools Performance 面板）。

### 4.2 状态同步一致性测试
- **场景测试**:
    1. 手动修改配置文件 $\rightarrow$ 验证前端 UI 是否自动更新。
    2. 通过后端命令切换 Agent $\rightarrow$ 验证前端当前对话窗口是否能立即反映 Agent 身份变化。

### 4.3 兼容性测试
- 验证在消息合并策略开启时，消息的完整性和顺序是否依然正确，是否存在丢失字符的情况。
