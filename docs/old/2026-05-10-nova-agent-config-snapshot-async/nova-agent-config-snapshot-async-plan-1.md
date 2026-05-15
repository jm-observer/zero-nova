# Plan 1: 接口异步化（必做）

## 前置依赖
- 无

## 本次目标
- 将 `AgentApplication::config_snapshot` 从同步方法改为异步方法。
- 移除 `self.config.blocking_read()`，确保配置快照读取不会阻塞 Tokio worker。
- 修复所有调用点，保证编译通过且行为不变。

## 涉及文件
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/` 下所有调用 `config_snapshot` 的文件
- 可能涉及引用 `Arc<dyn AgentApplication>` 的上层适配层

## 详细设计
### 1. trait 签名改造
- 变更：`fn config_snapshot(&self) -> Result<Value>` 改为 `async fn config_snapshot(&self) -> Result<Value>`。
- 由于 trait 已使用 `async_trait`，该改造与现有风格一致，不需要额外引入机制。

### 2. 实现改造
- 将当前实现改为：
  - `let config = self.config.read().await;`
  - `serde_json::to_value(&*config).context("Failed to serialize config")`
- 该实现能保证：
  - 不阻塞 executor 线程。
  - 返回值仍然是同一份 `AppConfig` 的一致性快照。
- 这一版不额外引入缓存，保持改动面最小，先解决 correctness 问题。

### 3. 调用链改造原则
- 所有 `config_snapshot()` 调用改为 `config_snapshot().await`。
- 若调用者当前是同步函数：
  - 优先将其升级为 `async fn` 并向上透传。
  - 禁止使用 `Handle::block_on`、`futures::executor::block_on`、`spawn_blocking` 包装该读取逻辑。
- 若某个边界层必须保留同步签名，应重新评估该边界是否设计合理，而不是在内部桥接异步调用。

### 4. 错误与行为约束
- 保持错误上下文 `Failed to serialize config` 不变，避免影响上层排障。
- 返回类型保持 `serde_json::Value`，避免 Plan 1 扩大接口变更范围。

### 5. Plan 1 的局限
- Plan 1 解决的是“阻塞执行器”的 correctness 问题，不保证读取热路径最优。
- 若 `config_snapshot` 被 UI 轮询、健康检查或 provider 状态接口高频调用，仍建议进入 Plan 2。

## 测试案例
- 正常路径：`config_snapshot().await` 返回 JSON，字段与当前 `AppConfig` 一致。
- 并发路径：并发执行多次 `config_snapshot().await` 与一次或多次 `update_config(...).await`，读写均成功且不 panic。
- 回归检查：全局检索不再存在 `config.blocking_read()`。
- 编译回归：所有 `config_snapshot` 调用端完成 `await` 适配。
