# nova-agent-config-snapshot-async

## 时间
- 创建时间：2026-05-10
- 最后更新：2026-05-10

## 项目现状
- `AgentApplication` trait 当前定义 `fn config_snapshot(&self) -> Result<Value>`，属于同步接口。
- `AgentApplicationImpl::config_snapshot` 在 `crates/nova-agent/src/app/application.rs:383` 内部使用 `self.config.blocking_read()`。
- 当前 `update_config` 的顺序是“先写磁盘，再获取 `self.config.write().await` 更新内存”，位于 `crates/nova-agent/src/app/application.rs:388`。
- `config_snapshot` 当前仅有一处实现，但 trait 变更会影响所有 `Arc<dyn AgentApplication>` 的调用端，因此需要全链路编译修复。

## 整体目标
- 彻底移除配置快照路径中的同步阻塞锁，避免在 Tokio runtime worker 上阻塞执行器。
- 保持对外语义稳定：仍返回当前配置的 JSON 快照，错误语义保持清晰。
- 将高频读取路径优化为原子快照缓存，避免读锁竞争与重复序列化。

## 推荐结论
- 必做项：完成接口异步化，将 `config_snapshot` 改为 `async fn` 并移除 `blocking_read`。
- 确定实施：Plan 2 直接采用原子快照缓存，读取路径不再依赖 `self.config` 读锁，也不再重复执行 `serde_json::to_value(...)`。
- 不建议：使用 `block_on`、`spawn_blocking`、单独开线程桥接同步 trait，这些做法会保留或转移阻塞问题，不属于彻底修复。

## Plan 拆分
| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 接口异步化：trait 与实现改造，移除 `blocking_read` 并修复调用链 | 无 | 1 | 已完成 |
| Plan 2 | 原子快照缓存：固定采用原子替换缓存，消除高频读取锁竞争与重复序列化 | Plan 1 | 2 | 已完成 |
| Plan 3 | 测试与验收：并发验证、静态防回归、修复流程闭环 | Plan 1、Plan 2 | 3 | 已完成 |

## 风险与待定项
- 风险 1：Plan 1 会修改 trait 签名，所有同步包装层、HTTP/WebSocket 处理层都可能需要补 `await`。
- 风险 2：Plan 2 需要新增原子快照依赖，必须放到 workspace 根 `Cargo.toml` 的 `[workspace.dependencies]` 统一声明。
- 风险 3：`update_config` 必须严格控制“磁盘态、内存态、快照态”的更新顺序，避免短暂分叉被测试遗漏。
- 待定项：无。Plan 2 的技术选型已固定为原子快照缓存。

## 验收标准
- `crates/nova-agent/src/app/application.rs` 中不再出现 `blocking_read`。
- `config_snapshot` 的任何实现与调用链均不包含同步阻塞桥接。
- 快照读取路径不再依赖 `self.config.read().await`，而是直接读取原子快照缓存。
- `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace` 全部通过。
