# Phase 4：会话控制层骨架

> 前置依赖：Phase 1-3  
> 基线设计：`docs/conversation-control-plane-design.md`

## 1. 目标

这是从“普通 chat gateway”走向“可控 agent 系统”的第一阶段。  
第四阶段不直接做完整 workflow，而是先把**会话控制层骨架**接到当前 `src` 结构上。

核心目标：

- 在 `gateway/router` 之前增加一层 turn 解释
- 让 session 不只知道 `history`，还知道：
  - 当前 active agent
  - 当前 workflow
  - 当前 pending interaction

## 2. 当前问题

当前 `src` 的路由方式本质上只有一种：

- 收到 `chat`
- 直接调 `agent.run_turn()`

这无法覆盖：

- 多 agent 点名
- 自然语言确认
- 长流程任务
- workflow 续接

所以第四阶段的重点不是“多加几个 handler”，而是引入新的顶层状态机入口。

## 3. 本 phase 范围

### 3.1 要做

- 定义 `SessionState`
- 定义 `TurnRouter`
- 定义 `PendingInteraction`
- 在 `chat` 路径前插入 turn 解释层

### 3.2 不做

- 不做完整 solution workflow
- 不做 skill 路由
- 不做 agent 自动协商

## 4. 设计结论

### 4.1 新增 `SessionState`

建议不要再让 session 只有 `history`。  
引入一个更完整的会话状态：

```rust
pub struct SessionState {
    pub active_agent: String,
    pub pending_interaction: Option<PendingInteraction>,
    pub workflow: Option<WorkflowState>,
}
```

这部分可以先挂在 `Session` 上，哪怕最开始字段都是 `Option`。

### 4.2 新增 `TurnRouter`

建议优先级：

1. 解析 `pending_interaction`
2. 解析 agent 点名
3. 解析 workflow 续接
4. 否则按普通 chat/new task 处理

### 4.3 `handle_chat()` 不再直接等于“执行 LLM”

当前 `handle_chat()` 既是入口，又是执行器。  
Phase 4 后，它应拆成：

- `route_turn(...)`
- `execute_turn(...)`

## 5. 实现细节

### 5.1 新增控制层模块

建议新增：

```text
src/control/
├── mod.rs
├── turn_router.rs
├── interaction.rs
└── workflow.rs
```

初期只实现最小结构，不追求功能完整。

### 5.2 最小 `PendingInteraction`

建议先支持两类：

- `ApproveAction`
- `SelectOption`

足够验证自然语言确认这条链路。

### 5.3 `TurnRouter` 的最小输出

例如：

```rust
pub enum TurnIntent {
    ResolvePendingInteraction,
    AddressAgent,
    ContinueWorkflow,
    ExecuteChat,
}
```

## 6. 测试方案

### 6.1 单元测试

覆盖：

- session 有 pending interaction 时，`继续` 被路由到 interaction resolver
- `OpenClaw 在不在` 被识别为 agent addressing
- 普通问题落到 `ExecuteChat`

### 6.2 集成测试

最小覆盖：

- 先挂起一个 interaction，再发送自然语言确认
- 无 interaction 时发送相同文本，不应被误判

## 7. 风险点

### 7.1 在现有 `router.rs` 里继续堆 if/else

应新增控制层模块，不要继续把控制逻辑塞回 gateway router。

### 7.2 workflow / interaction 状态只存在 prompt 里

关键状态必须落在 runtime，不然这个 phase 等于没做。

## 8. 完成定义

- `Session` 已具备控制层扩展位
- `TurnRouter` 已接入 chat 入口
- 最小 `PendingInteraction` 能工作

## 9. 给下一阶段的交接信息

Phase 5 会在这层控制骨架上，接入 workflow 与 multi-agent，而不是再回头改 session 结构。
