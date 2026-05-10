# Plan 1: 并发状态与锁位点盘点

## 前置依赖
无

## 本次目标
建立 `nova-agent` 锁使用现状的“可执行清单”，明确迁移边界与优先级：
- 哪些文件/结构体在异步路径中使用同步锁。
- 哪些锁获取存在 `unwrap/expect` panic 风险。
- 哪些函数持锁范围过大，包含潜在耗时逻辑。

## 涉及文件
- `crates/nova-agent/src/conversation/cache.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/tool.rs`
- `crates/nova-agent/src/conversation/model.rs`
- `crates/nova-agent/src/app/types.rs`

## 详细设计
1. 盘点维度
- 维度 A：锁类型（`std::sync::RwLock/Mutex`、`tokio::sync::RwLock/Mutex`）。
- 维度 B：调用上下文（async 函数、sync 函数、测试代码）。
- 维度 C：错误处理（`unwrap/expect`、`?`、显式错误映射）。
- 维度 D：持锁范围（仅字段访问 / 含序列化 / 含 IO 或外部调用准备）。

2. 产出物
- 锁位点清单表（文件、行号、结构体字段、锁类型、改造优先级）。
- 风险标签：
  - P0：async 链路 + `std::sync` + `unwrap/expect`。
  - P1：async 链路 + `std::sync`（无 panic 但阻塞风险）。
  - P2：test-only 或非热点路径。

3. 优先级规则
- 优先处理高频读写且跨模块共享的会话状态锁。
- 优先处理在请求主链路中持锁并进行格式化/转换的代码。
- 最后处理仅测试或工具链辅助路径。

4. 验收标准
- 形成可追踪的位点清单（不少于“字段级别”）。
- 明确 Plan 3 的首批迁移目标文件与原因。

### 4.1 锁位点清单（字段级）
| 文件 | 位点（字段/函数） | 锁类型 | 上下文 | 当前错误处理 | 持锁范围评估 | 风险 |
|---|---|---|---|---|---|---|
| `crates/nova-agent/src/conversation/cache.rs` | `SessionCache.sessions`（`get/insert/remove/list`） | `std::sync::RwLock<HashMap<...>>` | `SessionService` async 主链路会调用 | `read/write().unwrap()` | 短临界区（Map 访问） | P0 |
| `crates/nova-agent/src/conversation/service.rs` | `SessionService.loading` | `std::sync::RwLock<HashMap<String, Vec<oneshot::Sender<_>>>>` | async `get()` 冷加载去重 | `write().unwrap_or_else(...)` | 短临界区，但在高并发热路径 | P1 |
| `crates/nova-agent/src/conversation/service.rs` | `Session.control/name/history/cancellation_token/title_state` | `std::sync::RwLock<_>`（创建与读写） | 几乎全部会话 async 主链路 | 多为 `unwrap_or_else(...)`，少量测试 `unwrap()` | 多处包含 clone/序列化前构造，部分范围偏大 | P1 |
| `crates/nova-agent/src/app/conversation_service.rs` | `resolve_run_models()` 读取 `session.control` | `std::sync::RwLock`（经 `Session`） | async turn 主链路 | `read().unwrap()` | 字段读取为主，但有默认模型拼装 | P0 |
| `crates/nova-agent/src/app/conversation_service.rs` | `execute_agent_turn()` 中读取 `control`、构造 `snapshot_internal` | 同上 | async turn 主链路 | `read().unwrap_or_else(...)` | 含 prompt/snapshot 构建与序列化映射，范围偏大 | P1 |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | `config` 字段 | `std::sync::RwLock<AppConfig>` | async API 服务方法 | `read().map_err(...)` | 读后 clone，范围可控 | P1 |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 读取 `session.control`（inspect/runtime/reload/list/override） | `std::sync::RwLock`（经 `Session`） | async API 主链路 | 多处 `read().unwrap()` | 多数短读；个别函数跨逻辑段重复持锁 | P0 |
| `crates/nova-agent/src/tool.rs` | `ToolRegistry.tools/deferred` | `tokio::sync::Mutex<_>` | async 工具注册/解析 | `try_lock().expect(...)` | 临界区短，但 panic 风险存在 | P1 |
| `crates/nova-agent/src/tool.rs` | `ToolExecutionContext.turn_read_state/shared_environment` | `tokio::sync::RwLock<_>` | async 执行上下文 | 依赖调用方 | 未发现阻塞锁问题 | P2 |
| `crates/nova-agent/src/conversation/model.rs` | 纯数据结构 | 无锁 | N/A | N/A | 无持锁逻辑 | P2 |
| `crates/nova-agent/src/app/types.rs` | 纯 DTO/事件映射 | 无锁 | N/A | N/A | 无持锁逻辑 | P2 |

### 4.2 `unwrap/expect` 风险摘录（生产路径）
- `crates/nova-agent/src/conversation/cache.rs`：4 处 `read/write().unwrap()`。
- `crates/nova-agent/src/app/conversation_service.rs`：`resolve_run_models()` 使用 `session.control.read().unwrap()`。
- `crates/nova-agent/src/app/agent_workspace_service.rs`：多处 `session.control.read().unwrap()`。
- `crates/nova-agent/src/tool.rs`：`lock_tools/lock_deferred` 使用 `try_lock().expect(...)`。

### 4.3 持锁范围偏大位点（优先收敛）
- `crates/nova-agent/src/app/conversation_service.rs`：`execute_agent_turn()` 中控制态读取与 prompt/snapshot 构建耦合。
- `crates/nova-agent/src/conversation/service.rs`：`update_runtime_state()` 与 `reload_system_prompt()` 在写锁内同时处理多字段更新与部分衍生计算。
- `crates/nova-agent/src/app/agent_workspace_service.rs`：`reload_session_system_prompt()` 在单函数中多段读取控制态，建议抽成“快照读取后释放锁”。

### 4.4 Plan 3 首批迁移目标（已明确）
1. `crates/nova-agent/src/conversation/cache.rs`
- 原因：P0，集中且改造面小，可快速建立 `tokio::sync::RwLock` 基线模式。
2. `crates/nova-agent/src/app/conversation_service.rs`
- 原因：请求主链路，存在 `unwrap` 与偏大持锁范围，是收益最高位点。
3. `crates/nova-agent/src/app/agent_workspace_service.rs`
- 原因：多 API 入口重复 `session.control.read().unwrap()`，可批量收敛错误处理与持锁时长。
4. `crates/nova-agent/src/conversation/service.rs`（第二批）
- 原因：影响面最大，需在 Plan 2 接口收敛后实施，避免一次性大改。

## 测试案例
- 静态检查用例：
  - 使用 `rg` 检索 `std::sync::{RwLock, Mutex}` 在 `src/` 中剩余位点。
  - 使用 `rg` 检索锁获取后 `unwrap/expect` 位点。
- 基线行为用例：
  - 在改造前记录关键会话流测试集（创建会话、切换 agent、读取上下文）通过情况，作为回归基线。

### Plan 1 交付检查结果
- 已完成字段级位点盘点与风险分级。
- 已明确 Plan 3 首批迁移目标与顺序。