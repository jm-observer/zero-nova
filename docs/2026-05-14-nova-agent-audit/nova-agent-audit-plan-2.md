# Plan 2：🟡 过度设计问题

## Plan 编号与标题

Plan 2：过度设计（同步/异步双写、双锁、双路径、AgentEvent 膨胀、Skill 层级混乱）

## 前置依赖

无（可与 Plan 1 并行评审，但修复建议在 Plan 1 完成后实施）

## 本次目标

识别并记录当前过度设计的具体表现，明确消除路径。

---

## 问题 4：同步/异步函数全面双写

整个 crate 中大量 I/O 函数均维护同步和异步两份近乎完全相同的实现：

| 位置 | 同步版本 | 异步版本 |
|------|---------|---------|
| `prompt/mod.rs` | `load_project_context()` | `load_project_context_async()` |
| `prompt/mod.rs` | `load_project_context_with_config()` | `load_project_context_with_config_async()` |
| `prompt/mod.rs` | `load_developer_project_prompt()` | `load_developer_project_prompt_async()` |
| `prompt/mod.rs` | `SystemPromptBuilder::from_config()` | `SystemPromptBuilder::from_config_async()` |
| `prompt/mod.rs` | `WorkflowStagePrompts::load_from_file()` | `WorkflowStagePrompts::load_from_file_async()` |
| `tool/registry.rs` | `resolve_deferred()` | `resolve_deferred_async()` |
| `tool/registry.rs` | `lock_state_startup_only()` | `lock_state_async()` |
| `tool/registry.rs` | `has_loaded_tool()` | `has_loaded_tool_async()` |
| `tool/registry.rs` | `load_deferred_by_category()` | `load_deferred_by_category_async()` |

**问题**：

- 这是一个 tokio 异步项目，`std::fs` 同步调用在 async 上下文中会阻塞 worker，违反 AGENTS.md 约束
- 代码量膨胀约 30-40%
- 维护成本翻倍，每次修改需同步两份

**建议**：统一为异步版本。如启动期确实需要同步调用，使用 `tokio::task::spawn_blocking`；不需要在库层面维护双版本。

---

## 问题 5：ToolRegistry 双锁策略过度复杂

`ToolRegistry` 采用 `Mutex<RegistryState>` + `RwLock<Arc<RegistrySnapshot>>` 双锁架构，并维护了 6 个 lock 辅助方法：

```
lock_state_startup_only()        — try_lock() + panic
lock_state_async()               — .await
lock_snapshot_startup_only()     — try_read() + panic
lock_snapshot_async()            — .await
refresh_snapshot_locked_startup_only() — try_write() + panic
refresh_snapshot_locked_async()  — .await
```

**问题**：

- `try_lock() + panic` 违反 AGENTS.md 禁止 `.unwrap()` 规则
- `startup_only` 在语义上暗示"启动时调用"，但实际在 `tool_definitions()`（高频调用路径）中也被使用
- Snapshot 刷新策略本身有价值（读多写少），但命名误导了使用方式

**建议**：统一为 async 方法；`startup_only` 系列在初始化完成后彻底移除或重命名为明确的 `try_*` 变体并用 `anyhow::Result` 返回错误而非 panic。

---

## 问题 6：Skill 系统三层模型混乱

Skill 模块存在多层并发抽象：

- `Skill`（旧）与 `SkillPackage`（新）共存，`SkillRegistry` 同时持有两个 `Vec`，注释说"逐步替代"但无迁移计划
- `CapabilityPolicy` 中含有 cache 预算参数（`cache_section_min_tokens`、`system_prompt_cache_target`），这属于 Provider 层面的关注点，不应出现在 Skill Policy 中
- `SkillInvocationLevel`、`SkillSwitchResult`、`SkillRouteDecision` 被定义在 `prompt/mod.rs` 中，但它们与 prompt 构建无关
- `ActiveSkillState::decide_active_skill()` 是空壳（注释 `// 阶段一：返回 None`），但上层已围绕它建立了完整的 `prepare_turn` 流程

**建议**：

1. 确立迁移截止节点，将所有 `Skill` 引用切换为 `SkillPackage`，删除旧 `Skill` 类型
2. 将 `SkillInvocationLevel`、`SkillSwitchResult`、`SkillRouteDecision` 移入 `skill/types.rs`
3. 将 cache 相关参数从 `CapabilityPolicy` 迁入 Provider 配置层

---

## 问题 7：AgentEvent 变体膨胀

`AgentEvent` 枚举有 **18 个变体**，混合了流式输出、生命周期、Skill、Task、调试等完全不同级别的事件：

```
流式输出：  TextDelta、ThinkingDelta、LogDelta
生命周期：  TurnComplete、IterationLimitReached
Skill：    SkillActivated、SkillSwitched、SkillExited、SkillRouteEvaluated、SkillInvocation、SkillLoaded、ToolUnlocked
Task：     TaskCreated、TaskStatusChanged、BackgroundTaskComplete
Agent：    AgentSwitched、AssistantMessage
调试：     LoopGuardTriggered（携带 8 个字段）、OrchestrationProgress
```

**问题**：`SkillRouteEvaluated`、`SkillInvocation`、`ToolUnlocked` 在代码中几乎没有被实际发送。`LoopGuardTriggered` 携带 8 个字段，更适合作为结构化日志而非事件通道消息。

**建议**：将调试类事件（`LoopGuardTriggered`、`OrchestrationProgress`）改为 `log::debug!` 输出，从枚举中移除；清理未使用变体。

---

## 问题 8：双 Agent 路径 — `run_turn` vs `run_turn_with_context`

`AgentRuntime` 存在两套完整的 turn 执行路径，由 `use_turn_context: bool` 开关控制：

| 路径 | 入口 | 状态 |
|------|------|------|
| 旧路径 | `run_turn()` → `run_turn_with_model_config()` | 应废弃 |
| 新路径 | `prepare_turn()` + `run_turn_with_context()` → `run_turn_with_context_and_model_config()` | 正式路径 |

**问题**：新路径已存在相当长时间（经历了 Phase 2、3），旧路径仍然保留，导致每个 turn 执行相关的改动都需要考虑两条路径。

**建议**：删除旧路径，彻底移除 `use_turn_context` 开关，`run_turn` 直接委托 `run_turn_with_context`。
