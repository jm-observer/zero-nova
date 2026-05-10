# Plan 2: 锁抽象与接口收敛设计

## 前置依赖
- Plan 1: 并发状态与锁位点盘点

## 本次目标
定义统一并发模型和接口边界，减少迁移期重复改动：
- 确定“业务状态锁”统一到 `tokio::sync::RwLock`。
- 定义锁访问模式与错误处理规范。
- 约束持锁范围，避免在锁内执行潜在耗时逻辑。

## 涉及文件
- `crates/nova-agent/src/conversation/model.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/conversation/cache.rs`

## 详细设计
1. 统一类型策略
- 共享会话可变状态：`Arc<tokio::sync::RwLock<T>>`。
- 串行化关键区（若读写锁不合适）：`Arc<tokio::sync::Mutex<T>>`。
- 保留同步锁的唯一例外：纯同步、短生命周期、非 async 链路且有充分注释说明（本轮默认不新增例外）。

2. 访问模式规范
- 读路径：`let snapshot = { let guard = state.read().await; guard.clone_or_extract() };` 然后在锁外处理。
- 写路径：在锁外完成参数校验与准备，锁内仅执行最小状态变更。
- 禁止在持锁期间执行：文件 IO、网络请求、复杂序列化、跨服务调用。

3. 错误处理规范
- 锁获取失败（理论上 `tokio::sync` 不存在 poison）不再使用 panic 分支。
- 与锁相关的业务失败统一返回 `anyhow::Result`，必要时 `.context("...")` 标注业务语义。

4. 接口影响控制
- 若 `read()/write()` 变为 async 导致函数签名变化，优先向上提升 async，避免 `block_in_place`。
- 对外公共接口尽量保持不变；若不可避免，优先在 crate 内部先适配并集中修改调用点。

5. 验收标准
- 形成“锁类型与访问模式”规范段落并在相关模块实现一致。
- Plan 3 迁移时不再讨论锁策略，只执行既定规范。

## 测试案例
- 编译层面：确保签名调整后无阻塞适配代码（如 `block_in_place`）引入。
- 行为层面：会话读写一致性测试在高并发下结果稳定（无丢写、无状态回退）。