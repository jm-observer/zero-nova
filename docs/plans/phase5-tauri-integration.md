# Phase 5：Workflow 与 Multi-Agent 接入

> 前置依赖：Phase 4  
> 基线设计：`docs/conversation-control-plane-design.md`

## 1. 目标

第五阶段把控制层骨架变成真正可用的行为系统，重点落两件事：

1. **workflow**
2. **multi-agent**

这两件事都建立在当前 `src` 已有的：

- gateway
- session
- runtime
- control plane skeleton

之上。

## 2. 本 phase 范围

### 2.1 要做

- 增加 `WorkflowState`
- 落地一个通用 `solution-workflow`
- 增加 `AgentRegistry / AgentDescriptor`
- 支持自然语言 agent addressing 与切换

### 2.2 不做

- 不做 skill 系统
- 不做复杂 agent 间协商
- 不做多 workflow 并行

## 3. 设计结论

### 3.1 先做一个通用 workflow，不做垂直 skill

第一版建议只落一个：

- `solution-workflow`

用于覆盖：

- TTS
- 文生图
- OCR
- 其他方案搜索、选型、部署、测试

### 3.2 workflow 的状态必须显式

建议最小阶段：

- `Discover`
- `Compare`
- `AwaitSelection`
- `AwaitExecutionConfirmation`
- `Executing`
- `AwaitTestInput`
- `Completed`

### 3.3 多 agent 先做“点名和切换”，不做自动协商

建议先支持：

- “OpenClaw 在不在”
- “让 OpenClaw 处理”
- “OpenClaw，帮我看这个”

而不是一上来做 agent 之间自动 delegation。

## 4. 实现细节

### 4.1 AgentRegistry

建议新增：

```rust
pub struct AgentDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
}
```

初期哪怕只有 `nova` 一个真实 agent，也要把抽象先立起来。

### 4.2 WorkflowState

建议挂在 `SessionState` 上，不挂在全局状态上。  
原因：workflow 是会话级，而不是服务级。

### 4.3 Interaction 与 Workflow 联动

当 workflow 进入：

- `AwaitSelection`
- `AwaitExecutionConfirmation`

应自动生成 `PendingInteraction`，而不是让模型自由发挥。

## 5. 测试方案

### 5.1 Workflow 测试

覆盖：

- 新任务进入 `solution-workflow`
- 方案选择推进阶段
- 部署确认推进阶段

### 5.2 Multi-agent 测试

覆盖：

- 点名已存在 agent
- 点名不存在 agent
- 切换 active agent

## 6. 风险点

### 6.1 把 workflow 做成 prompt 约定而不是 runtime 状态

这会让阶段不可观测，也无法做稳定确认。

### 6.2 多 agent 直接建立在 `agents.switch` 协议之上

协议切换只是外部接口，不是内部控制模型本身。  
内部仍需要 `AgentContext / AgentRegistry`。

## 7. 完成定义

- `solution-workflow` 可运行
- agent addressing 与切换可运行
- interaction 与 workflow 已打通

## 8. 给下一阶段的交接信息

Phase 6 将在已有 control plane、workflow、multi-agent 之上，再引入 skill 与历史压缩，而不是重做控制模型。
