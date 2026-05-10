# Plan 3: 渐进迁移实现（核心模块先行）

## 前置依赖
- Plan 1: 并发状态与锁位点盘点
- Plan 2: 锁抽象与接口收敛设计

## 本次目标
分批将高优先模块迁移到统一锁模型，保持每批改动可验证、可回滚：
- 第一批：`conversation/cache` 与 `app/conversation_service`。
- 第二批：`app/agent_workspace_service`。
- 第三批：其余关联模块与调用点收口。

## 涉及文件
- `crates/nova-agent/src/conversation/cache.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/model.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/tests/integration/session_project_runtime.rs`
- `crates/nova-agent/tests/integration/session_project_lineage.rs`

## 详细设计
1. 批次策略
- 批次 1（低耦合）：先迁移 `conversation/cache.rs`，验证锁类型切换不改变缓存语义。
- 批次 2（主链路）：迁移 `app/conversation_service.rs` 的状态读写路径，消除 `read().unwrap()`。
- 批次 3（外围扩展）：迁移 `agent_workspace_service` 及少量调用方签名。

2. 迁移动作模板
- 字段类型替换：`std::sync::RwLock<T>` -> `tokio::sync::RwLock<T>`。
- 调用替换：`read().unwrap()` -> `read().await`；`write().unwrap()` -> `write().await`。
- 函数签名升级：必要时 `fn` -> `async fn`，并同步调用链。
- 持锁范围收缩：在锁外准备数据，在锁内只做读取或原子更新。

3. 兼容性控制
- 不调整外部协议结构体字段与序列化格式。
- 不在同一提交混入业务逻辑重构；仅做并发模型迁移与最小必要适配。

4. 回滚策略
- 每批次独立提交，若出现行为回归可按批次回退。
- 保留原测试用例并新增并发场景用例，确保语义对齐。

5. 验收标准
- `src/` 主链路中不再出现 `std::sync::RwLock` 的会话状态使用。
- 迁移模块锁相关 `unwrap/expect` 清零（测试代码除外）。
- 修复流程一次性通过。

## 测试案例
- 正常路径：
  - 创建会话、读取会话、更新控制状态、再次读取一致。
- 边界条件：
  - 并发多任务同时读取同一会话状态，结果一致无 panic。
- 异常路径：
  - 会话不存在、路径无效等错误可返回业务错误而非 panic。