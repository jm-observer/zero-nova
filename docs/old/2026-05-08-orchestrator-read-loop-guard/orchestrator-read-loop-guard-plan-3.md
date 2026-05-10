# Plan 3: 观测、配置与测试补齐

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 3: 观测、配置与测试补齐 |
| 前置依赖 | Plan 1、Plan 2 |
| 本次目标 | 让循环保护机制具备可调阈值、可回放的日志事件和足够的自动化测试，避免修复后只能“感觉上变好了”。 |
| 涉及文件 | `crates/nova-agent/src/config.rs`、`crates/nova-agent/src/event.rs`、`crates/nova-agent/src/agent.rs`、`crates/nova-agent/tests/integration/*`、必要时补充 `.nova/config.toml` 示例 |

## 详细设计

### 1. 配置项设计

建议在 `gateway` 下新增具名配置段，例如：

```toml
[gateway.loop_guard]
enabled = true
max_consecutive_duplicate_tool_calls = 2
max_stalled_iterations = 3
duplicate_read_mode = "warn_then_reject"
iteration_trim_ratio = 0.85
```

配置目标：

- 便于不同模型调参
- 便于线上快速关闭或放宽保护
- 避免把阈值散落在代码里

### 2. 观测设计

建议记录三类观测：

1. **日志**
   - 适合快速排障
   - 应带结构化关键信息

2. **事件**
   - 适合 UI 或调试面板消费
   - 可显示“本轮因重复读取被阻断”

3. **测试快照**
   - 适合验证输出文案和行为稳定

建议观测字段：

- `session_id`
- `tool_name`
- `canonical_target`
- `duplicate_count`
- `stalled_iteration_count`
- `decision`
- `reason_code`

### 3. 协议与兼容

如果新增事件类型成本过高，首版可以复用 `SystemLog` 或 `LogDelta`，但消息文案必须稳定、可断言。

若新增协议事件，建议保持前后兼容：

- 老客户端忽略新事件
- 新客户端可视化展示循环保护命中

### 4. 自动化测试矩阵

测试需要同时覆盖：

- 纯单元测试：规则判断是否正确
- runtime 测试：单轮 iteration 是否被熔断
- 工具测试：`Read` 是否按场景切换原文/摘要/提示
- 集成测试：orchestrator skill 场景下是否不再出现连续重复读取

### 5. 回归关注点

重点防止以下回归：

1. 合法分页读取被误杀
2. tool result 被裁剪后导致上下文断裂
3. warning 文案过弱，模型继续无限重试
4. 不同 agent 使用相同 runtime 时行为不一致
5. turn-scoped 读取状态误挂到全局 `ReadTool`，导致跨 session 串扰

## 测试案例

1. 配置关闭 `loop_guard`：行为回退为当前实现。
2. 配置 `warn_then_reject`：第二次重复警告，第三次拒绝。
3. 配置只警告不拒绝：重复读取仍执行，但原文被摘要化。
4. 新增事件启用时，前端可收到命中事件；禁用时不影响原有 turn 流程。
5. orchestrator 复现用例回放：同一文件不会再被连续读取十余次。
6. developer 普通会话中读取同一文件两次但中间有实质分析：不会被误判为 stalled turn。
7. 并发 session 用例：不同 session 的 read history 与 duplicate counter 完全隔离。
