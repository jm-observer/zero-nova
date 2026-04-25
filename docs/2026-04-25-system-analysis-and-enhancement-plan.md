# Zero-Nova 功能梳理与增强设计方案

**日期**: 2026-04-25
**当前状态**: 架构已梳理，问题点已识别，设计方案制定中。

## 1. 当前功能总结 (Current State)

### 后端 (Rust)
Zero-Nova 采用分层架构，实现了高度解耦的 Agent 运行机制：
- **`nova-protocol`**: 定义了基于 Envelope 模式的 JSON 通信标准，支持请求/响应及异步事件推送。
- **`nova-gateway-core`**: 实现路由分发（Router）与事件桥接（Bridge），支持 WebSocket 和 Stdio 两种通信渠道。
- **`nova-core`**: 核心 Agent 运行时，实现了基于 ReAct 模式的循环：LLM 交互 $\rightarrow$ 工具解析 $\rightarrow$ 并行工具执行 $\rightarrow$ 结果回传。

### 前端 (TypeScript/Tauri)
基于 Tauri 的桌面应用，提供可视化的对话交互：
- **通信层**: 通过 `gateway-client.ts` 建立 WebSocket 连接，解析并分发后端推送的流式消息。
- **视图层**: 实现聊天窗口、思考过程展示、工具执行状态反馈及配置管理。

---

## 2. 识别的问题 (Identified Issues)

### 🔴 稳定性与规范问题
1. **违反规范的 `unwrap()`**: 在 `crates/nova-core/src/agent.rs:227` 存在潜在的 `panic` 风险。
2. **逻辑耦合度高**: `nova-core` 中的 `run_turn` 函数过于臃肿，难以测试且违反单一职责原则。
3. **工具错误处理过于宽泛**: 严重的系统级错误被包装为 `ToolResult` 传给 LLM，可能导致 Agent 陷入无效的修复循环。

### 🟡 衔接与性能问题
1. **协议覆盖不全**: 后端 `router.rs` 中存在部分 `Not implemented` 分支，前端缺乏对应的优雅错误提示。
2. **流式渲染压力**: 高频的 `ThinkingDelta` 和 `TextDelta` 推送可能在极端情况下导致前端 UI 重绘压力过大。
3. **状态同步延迟**: 依赖 `mpsc` 通道进行事件转发，虽然灵活但可能存在微小的调度延迟。

---

## 3. 下一步增强设计 (Proposed Enhancements)

### 🚀 目标 1：提升系统健壮性 (Robustness & Refactoring)
- **安全重构**: 消除 `nova-core` 中的 `unwrap()`，改为防御性编程。
- **职责拆分**: 将 `AgentRuntime::run_turn` 拆分为 `MessageStreamHandler` (处理 LLM 流) 和 `ToolExecutionCoordinator` (处理工具并发调度)。
- **错误分级**: 引入 `ToolErrorSeverity`，区分“业务逻辑错误”（传给 LLM）和“系统级故障”（直接中断并通知前端）。

### 🚀 目标 2：优化前后端“丝滑”衔接 (Seamless Integration)
- **协议完善**: 在 `nova-protocol` 中定义显式的 `ErrorEnvelope`，包含错误码和人类可读的消息。
- **前端增强**:
    - 实现 `gateway-client.ts` 的异常捕获机制，针对“未实现”或“系统错误”提供 Toast 提示。
    - 引入 **消息聚合 (Message Batching)**：前端在处理极高频的 Delta 推送时，采用 requestAnimationFrame 进行节流渲染。
- **状态实时同步**: 增加 `SessionStateUpdate` 事件，当后端 Agent 状态、配置或工具集发生变化时，主动推送到前端更新 UI。

### 🚀 目标 3：增强用户体验 (UX Improvement)
- **可视化进度**: 增强工具执行的 UI 反馈，例如显示工具的实时输出日志（如果工具支持 stdout 捕获）。
- **思考过程优化**: 在 UI 上更清晰地区分“模型思考”与“最终文本输出”。

---

## 4. 执行计划拆分 (Implementation Roadmap)

我们将任务拆分为以下四个阶段的 Plan：

### [Plan A] 代码质量与规范修复 (Refactor & Fix)
- [ ] 修复 `nova-core` 中的 `unwrap()` 问题。
- [ ] 对 `run_turn` 进行模块化拆分。
- [ ] 规范化日志记录，清理残留的 `println!`。

### [Plan B] 通信协议与错误处理增强 (Protocol & Error Handling)
- [ ] 扩展 `nova-protocol`，增加结构化的错误消息类型。
- [ ] 后端实现对未实现接口的标准化错误返回。
- [ ] 前端实现针对不同协议错误类型的 UI 提示机制。

### [Plan C] 性能优化与状态同步 (Performance & Sync)
- [ ] 前端实现流式消息的渲染节流机制。
- [ ] 后端增加 Agent 状态变更的事件推送逻辑。
- [ ] 前端实现对 Agent 状态变更的响应式更新。

### [Plan D] 用户体验深度增强 (UX Deep Dive)
- [ ] 优化工具执行过程中的可视化反馈。
- [ ] 完善思考过程的 UI 表现形式。
