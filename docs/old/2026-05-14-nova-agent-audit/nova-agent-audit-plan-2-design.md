# Plan 2 详细设计：消除过度设计与双路径实现

## 时间

2026-05-14

## Plan 编号与标题

Plan 2：过度设计收敛（同步/异步双写、ToolRegistry 双锁命名、Agent 双路径、AgentEvent 膨胀、Skill 层级混乱）

## 前置依赖

- Plan 1 中 `prompt`、`conversation/service`、`conversation/repository` 的超规模文件拆分已完成。
- 不新增依赖。
- 保持现有外部协议兼容，尤其是 Gateway / Desktop 已消费的事件不应在本 Plan 中破坏。
- 本 Plan 优先消除“同一行为的重复路径”和“未使用/误导性抽象”，不做业务规则重写。

## 当前状态概览

Plan 2 原始审计提出 5 类问题：

1. 同步/异步函数双写。
2. `ToolRegistry` 双锁与 `startup_only` 命名/语义混乱。
3. Skill 系统旧/新模型共存与路由类型归属混乱。
4. `AgentEvent` 变体膨胀。
5. `AgentRuntime` 中 `run_turn` 与 `run_turn_with_context` 双路径并存。

当前已知变化：

- `prompt` 模块已拆分，`prompt-refactor` 文档显示“消除同步/异步双写”已完成；因此本设计不再重复改 `prompt` I/O，后续只做核验。
- `conversation` 的 Plan 1 剩余拆分已落地，Plan 2 可以开始处理运行路径与抽象层问题。

## 本次目标（可验证）

1. `ToolRegistry` 不再对外暴露 `*_startup_only` 语义的高频路径方法。
2. `ToolRegistry` 中同步/异步重复方法收敛为单一路径，保留必要的非阻塞快照读取，但避免 `panic` 作为常规控制流。
3. `AgentRuntime` 删除旧 turn 执行路径，`run_turn` 统一走 `prepare_turn` + context 路径。
4. 移除或降级未使用的调试事件，`AgentEvent` 不再承载内部诊断细节对象。
5. Skill 路由相关类型从 `prompt` 迁移到 `skill` 层，`prompt` 只 re-export 兼容一轮或直接更新调用方。
6. 所有变更后运行：
   - `cargo fmt --all`
   - `cargo test -p nova-agent`
   - `cargo clippy --workspace -- -D warnings`
   - 如事件协议调整影响前端，再补充 `deskapp` 相关测试。

## 涉及文件

### 重点文件

- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/agent/runtime/tool_exec.rs`
- `crates/nova-agent/src/agent/turn_executor.rs`
- `crates/nova-agent/src/event.rs`
- `crates/nova-agent/src/loop_guard.rs`
- `crates/nova-agent/src/prompt/routing.rs`
- `crates/nova-agent/src/prompt/types.rs`
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/skill/types.rs`
- `crates/nova-agent/src/skill/model.rs`
- `crates/nova-agent/src/skill/registry.rs`
- `crates/nova-agent/src/config/models.rs`

### 可能受影响文件

- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/lib.rs`
- `crates/nova-protocol/src/**`
- `crates/nova-gateway-core/src/**`
- `deskapp/src/**`

---

## Plan 拆分与执行顺序

| 子计划 | 标题 | 目标 | 依赖 |
|--------|------|------|------|
| Plan 2.1 | 同步/异步双写核验与清理 | 确认 `prompt` 已完成；收敛 `ToolRegistry` 的双版本方法 | Plan 1 |
| Plan 2.2 | ToolRegistry 锁语义收敛 | 移除 `startup_only` 命名和 panic 控制流，保留快照优化 | Plan 2.1 |
| Plan 2.3 | AgentRuntime 单路径化 | 删除旧 `run_turn_with_model_config` 路径，`run_turn` 统一委托 context 路径 | Plan 2.1 |
| Plan 2.4 | AgentEvent 瘦身 | 清理未发送事件；将内部诊断改为日志或结构体封装 | Plan 2.3 |
| Plan 2.5 | Skill 类型归属收敛 | 路由类型迁入 `skill/types.rs`，明确 `Skill` 与 `SkillPackage` 迁移边界 | Plan 2.3 |

说明：不建议一次性修改所有问题。优先清理 ToolRegistry 和 AgentRuntime 双路径，因为它们会影响后续 Skill 与事件路径。

---

# Plan 2.1：同步/异步双写核验与 ToolRegistry 方法收敛

## 目标

1. `prompt` 模块仅保留 async I/O source of truth，不恢复同步版本。
2. `ToolRegistry` 对外保留一组明确的 async API。
3. 若存在必须同步读取的调用点，改为在 async 上下文中 await；不引入 `block_on`。

## 当前问题

Plan 2 原始文档列出的 `prompt` 同步/异步双写在当前项目中应已完成。但 `tool/registry.rs` 仍需检查以下方法：

- `resolve_deferred()` / `resolve_deferred_async()`
- `lock_state_startup_only()` / `lock_state_async()`
- `has_loaded_tool()` / `has_loaded_tool_async()`
- `load_deferred_by_category()` / `load_deferred_by_category_async()`

## 设计

### API 收敛规则

| 原方法 | 新方法 | 处理 |
|--------|--------|------|
| `resolve_deferred()` | `resolve_deferred(...) async` 或保留 `resolve_deferred_async` 后重命名 | 删除同步版本 |
| `resolve_deferred_async()` | `resolve_deferred()` | 改名为标准方法 |
| `has_loaded_tool()` | `has_loaded_tool()` async | 调整调用方 await |
| `has_loaded_tool_async()` | `has_loaded_tool()` | 删除 `_async` 后缀 |
| `load_deferred_by_category()` | `load_deferred_by_category()` async | 调整调用方 await |
| `load_deferred_by_category_async()` | `load_deferred_by_category()` | 删除 `_async` 后缀 |

命名原则：当 crate 已是 tokio async 主体时，默认方法名就是 async，不需要 `_async` 后缀。

### 调用方迁移

用编译器驱动迁移：

1. 删除同步方法。
2. 将 `_async` 方法重命名为无后缀方法。
3. `cargo check -p nova-agent` 找到所有调用方。
4. 为调用方补 `.await?` 或 `.await`。

### 非目标

- 不改变 tool lazy loading 策略。
- 不改变 deferred tool 的加载顺序。
- 不改 tool definition schema。

## 测试

- `cargo test -p nova-agent tool::registry`
- 若无精确测试，则运行 `cargo test -p nova-agent tool`
- 手动编译验证所有调用点已 await。

---

# Plan 2.2：ToolRegistry 锁语义收敛

## 目标

移除 `startup_only` 命名和 `try_lock() + panic` 常规路径，同时保留读多写少场景下的 snapshot 优化。

## 当前问题

`ToolRegistry` 当前有两层状态：

```text
Mutex<RegistryState>
RwLock<Arc<RegistrySnapshot>>
```

这本身不是问题：

- `RegistryState` 负责真实可变状态。
- `RegistrySnapshot` 负责高频只读 tool definitions。

问题在于方法命名与错误处理：

- `lock_state_startup_only()` 名称暗示只能启动期调用，但实际可能出现在高频路径。
- `try_lock() + panic` 把锁竞争变成崩溃。
- `refresh_snapshot_locked_startup_only()` 命名误导。

## 设计

### 保留的内部方法

```rust
async fn lock_state(&self) -> MutexGuard<'_, RegistryState>;
async fn read_snapshot(&self) -> RwLockReadGuard<'_, Arc<RegistrySnapshot>>;
async fn refresh_snapshot_locked(&self, state: &RegistryState) -> Result<()>;
```

如确实需要非阻塞尝试版本，使用显式名称并返回 `Result`：

```rust
fn try_read_snapshot(&self) -> Result<Arc<RegistrySnapshot>>;
```

但第一阶段建议不要保留 try 版本，除非 `tool_definitions()` 必须同步。

### `tool_definitions()` 处理策略

有两种选择：

#### 方案 A：改成 async（推荐）

```rust
pub async fn tool_definitions(&self) -> Vec<ToolDefinition>
```

优点：彻底消除同步锁路径。
缺点：调用方需要 await。

#### 方案 B：保留同步快照读取，但不 panic

```rust
pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
    match self.snapshot.try_read() {
        Ok(snapshot) => snapshot.tool_definitions.clone(),
        Err(_) => {
            log::warn!("ToolRegistry snapshot is temporarily unavailable");
            Vec::new()
        }
    }
}
```

优点：调用方改动小。
缺点：锁竞争时返回空列表可能改变行为，不建议。

最终建议采用方案 A，确保行为稳定。

### Snapshot 刷新规则

所有会改变 registry state 的方法在 state 更新后调用：

```rust
self.refresh_snapshot_locked(&state).await?;
```

如果 `refresh_snapshot_locked` 不可能失败，可不返回 `Result`，但不应 panic。

## 测试

1. deferred tool 第一次调用前未加载。
2. resolve 后 snapshot 包含新 tool。
3. 并发调用 `tool_definitions()` 不 panic。
4. 并发 resolve 同一个 deferred tool 不重复注册或状态错乱。

---

# Plan 2.3：AgentRuntime 单路径化

## 目标

删除旧 turn 执行路径，统一为：

```text
run_turn()
  -> prepare_turn()
  -> run_turn_with_context()
  -> run_turn_with_context_and_model_config()
```

彻底移除 `use_turn_context: bool` 开关。

## 当前问题

`AgentRuntime` 中存在：

- 旧路径：`run_turn()` → `run_turn_with_model_config()`
- 新路径：`prepare_turn()` + `run_turn_with_context()` → `run_turn_with_context_and_model_config()`
- 配置开关：`use_turn_context: bool`

这导致以下问题：

- 修改 turn 执行必须同时理解两条路径。
- Tool 执行、Skill 路由、prompt 构建的行为可能分叉。
- 测试覆盖不清楚到底覆盖哪条路径。

## 设计

### 移除字段

从 runtime config / app config 中删除或停止使用：

```rust
use_turn_context: bool
```

如果配置文件中已有该字段，反序列化层可以暂时保留字段但标记忽略，避免旧配置直接失败：

```rust
#[serde(default)]
pub use_turn_context: Option<bool>
```

但业务代码不再读取它。

### `run_turn` 新实现

```rust
pub async fn run_turn(&self, input: TurnInput) -> Result<TurnOutput> {
    let turn_context = self.prepare_turn(&input).await?;
    self.run_turn_with_context(turn_context).await
}
```

实际签名以现有代码为准，不强行重命名参数。

### 删除旧路径

删除或内联：

- `run_turn_with_model_config()`
- 旧路径专用 prompt 构造逻辑
- 旧路径专用 tool registry resolve 调用
- 旧路径测试分支

### 保留兼容入口

外部仍可调用 `run_turn()`。不要求调用方改成 `run_turn_with_context()`。

### 验证重点

1. CLI one-shot 仍能完成一次 turn。
2. REPL chat 仍能流式输出。
3. Tool call 执行仍能产生相同 event。
4. Skill 命令或自动路由仍经过 context。
5. 历史裁剪仍在统一路径中执行一次。

## 测试

- `cargo test -p nova-agent agent::runtime`
- `cargo test -p nova-agent agent::turn_executor`
- `cargo test -p nova-cli cli`
- 可选手动：`cargo run -p nova-cli --bin nova_cli -- run "hello"`

---

# Plan 2.4：AgentEvent 瘦身

## 目标

减少 `AgentEvent` 中未使用或内部调试性质的变体，避免事件协议承载过细内部实现。

## 当前事件分类

| 类别 | 变体 | 处理建议 |
|------|------|----------|
| 流式输出 | `TextDelta`、`ThinkingDelta`、`LogDelta` | 保留 |
| 生命周期 | `TurnComplete`、`IterationLimitReached` | 保留 |
| Skill | `SkillActivated`、`SkillSwitched`、`SkillExited`、`SkillRouteEvaluated`、`SkillInvocation`、`SkillLoaded`、`ToolUnlocked` | 逐个核验发送方与前端消费方 |
| Task | `TaskCreated`、`TaskStatusChanged`、`BackgroundTaskComplete` | 保留 |
| Agent | `AgentSwitched`、`AssistantMessage` | 保留 |
| 调试 | `LoopGuardTriggered`、`OrchestrationProgress` | 优先降级为 log 或专用 progress 类型 |

## 设计

### 第一步：事件使用审计

用搜索确认每个变体：

1. 定义位置。
2. 发送位置。
3. Gateway 转换位置。
4. Desktop 消费位置。
5. 测试断言位置。

只有满足以下条件的事件才删除：

- 无发送方；或
- 仅测试中构造；或
- 仅内部调试用途，且无协议消费者。

### `LoopGuardTriggered` 处理

当前携带字段过多，建议改为结构化日志：

```rust
log::debug!(
    "loop guard triggered: session_id={session_id}, reason={reason}, ..."
);
```

如果 UI 确实需要展示，应改为更粗粒度事件：

```rust
AgentEvent::LogDelta { text: ... }
```

不建议继续用 8 字段枚举变体。

### `OrchestrationProgress` 处理

如前端需要展示 orchestration 进度，则保留；否则改为 `log::debug!`。

### Skill 事件处理

- `SkillRouteEvaluated`：如没有发送方，删除。
- `SkillInvocation`：如只用于实验路径，删除或合并到 `SkillActivated`。
- `ToolUnlocked`：如果 tool unlock UI 未消费，删除。

## 测试

- `cargo test -p nova-agent event`
- `cargo test -p nova-gateway-core`
- 如改 protocol：`cargo test --workspace`
- 前端如有事件 mapping：`pnpm test`（在 `deskapp` 下）

---

# Plan 2.5：Skill 类型归属收敛

## 目标

将 Skill 路由与 invocation 类型归入 `skill` 层，降低 `prompt` 对 Skill 决策的所有权错觉。

## 当前问题

`SkillInvocationLevel`、`SkillSwitchResult`、`SkillRouteDecision` 定义在 `prompt` 模块，但语义属于 Skill 路由/策略。

同时 `Skill` 与 `SkillPackage` 共存，`SkillRegistry` 同时持有旧/新模型，缺少迁移边界。

## 设计

### 路由类型迁移

迁移目标：

```text
prompt/routing.rs 或 prompt/types.rs
  -> skill/types.rs
```

迁移类型：

- `SkillInvocationLevel`
- `SkillSwitchResult`
- `SkillRouteDecision`
- 与它们强绑定的 helper 类型/方法

`prompt` 模块如果仍有调用方，可以临时 re-export：

```rust
pub use crate::skill::types::{SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult};
```

但推荐直接更新调用方 import 到 `crate::skill::types`，减少过渡层。

### `ActiveSkillState::decide_active_skill()` 处理

当前空壳逻辑不应继续支撑复杂流程。两种选择：

#### 方案 A：删除空壳并内联为 None

适用于短期：如果当前阶段确实不启用自动 active skill 决策，则删除函数，调用方明确写：

```rust
let active_skill = None;
```

优点：诚实表达未实现，减少伪抽象。

#### 方案 B：迁移到 SkillRouter 并补齐最小实现

适用于计划启用自动路由：

```rust
pub struct SkillRouter;
impl SkillRouter {
    pub fn decide_active_skill(...) -> Option<SkillRouteDecision> { ... }
}
```

本 Plan 建议采用方案 A，因为目标是消除过度设计，不是实现新路由。

### `Skill` 与 `SkillPackage` 共存处理

本 Plan 不强制一次删除旧 `Skill`，但必须明确迁移边界：

1. 统计旧 `Skill` 的构造与读取位置。
2. 如果仅 registry 内部使用，改为从 `SkillPackage` 派生展示信息。
3. 如果外部 API 暴露旧 `Skill`，先新增转换函数并逐步切换调用方。
4. 最终删除 `SkillRegistry` 中的旧 `Vec<Skill>`。

建议将旧模型删除拆为后续独立 Plan，因为影响范围可能大于事件/路径收敛。

### `CapabilityPolicy` cache 字段处理

字段：

- `cache_section_min_tokens`
- `system_prompt_cache_target`

归属上更接近 provider/prompt cache 策略。处理建议：

1. 本 Plan 不立即迁移配置字段，避免破坏配置兼容。
2. 新增注释标记为待迁移字段。
3. 后续独立 Plan 迁入 provider 或 prompt cache config。

## 测试

- `cargo test -p nova-agent skill`
- `cargo test -p nova-agent prompt`
- `cargo test -p nova-agent agent::runtime`

---

## 分阶段验收标准

### Plan 2.1 验收

- `tool/registry.rs` 不再同时存在同一行为的 sync 与 `_async` 双版本 public 方法。
- `prompt` 模块不恢复 sync I/O 双写。
- 所有调用方编译通过。

### Plan 2.2 验收

- `ToolRegistry` 中不再存在 `startup_only` 命名的方法。
- 常规路径中不再使用 `try_lock() + panic`。
- snapshot 刷新仍在注册/加载 deferred tool 后生效。

### Plan 2.3 验收

- `use_turn_context` 不再控制运行分支。
- 旧 `run_turn_with_model_config` 路径删除或不再被调用。
- `run_turn` 统一走 context 路径。

### Plan 2.4 验收

- 未发送的 `AgentEvent` 变体被删除。
- 内部调试事件优先转为 `log::debug!`。
- Gateway / Desktop 事件消费未破坏。

### Plan 2.5 验收

- Skill 路由类型定义归属 `skill/types.rs`。
- `prompt` 不再拥有 Skill 决策类型定义。
- 空壳 active skill 决策不再伪装成完整流程。

---

## 总体验证命令

```bash
cargo fmt --all
cargo test -p nova-agent
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

如事件协议或前端事件映射有改动，在 `deskapp/` 下执行：

```bash
pnpm test
```

---

## 风险与规避

| 风险 | 影响 | 规避 |
|------|------|------|
| ToolRegistry API 改 async 导致调用链大面积改动 | 中 | 先重命名内部方法，再由编译器定位调用方 |
| `tool_definitions()` 改 async 影响 trait 或协议接口 | 高 | 若 trait 要求同步，先保留同步 wrapper，但内部不得 panic |
| 删除旧 AgentRuntime 路径导致行为差异 | 高 | 先增加/保留 runtime 测试，确保 tool call、stream、history trim 走新路径 |
| AgentEvent 删除影响前端 | 高 | 删除前必须搜索 Gateway 与 deskapp 消费方 |
| Skill 类型迁移引发循环依赖 | 中 | 类型放 `skill/types.rs`，只依赖基础数据结构，避免依赖 `prompt` |
| 旧配置字段删除导致用户配置加载失败 | 中 | 配置字段先 deprecated/ignored，不立即删除 serde 字段 |

---

## 非目标

1. 不引入新依赖。
2. 不重写 ToolRegistry 架构。
3. 不改 tool schema。
4. 不改 SQLite 或 conversation 持久化。
5. 不实现新的 Skill 自动路由算法。
6. 不重构 Gateway 协议整体结构。
7. 不进行性能优化，除非是移除同步阻塞的直接结果。
8. 不处理 Plan 3 的 crate monolith、ToolDefinition 重复定义等长期问题。
