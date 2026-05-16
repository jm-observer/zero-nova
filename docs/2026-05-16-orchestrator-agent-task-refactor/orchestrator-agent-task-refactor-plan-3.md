# Plan 3: TaskStore 内部化并清理通用 Task 工具入口

## 前置依赖

Plan 1、Plan 2

## 任务目标

保留 `TaskStore` 作为编排状态模型，但取消其“模型可直接写入的通用任务工具”定位。收敛后的 `TaskStore` 只服务于 orchestrator 内部状态、前端展示与测试验证。

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/tool/builtin/task.rs`
  - `crates/nova-agent/src/tool/builtin/mod.rs`
  - `crates/nova-agent/src/tool/registry.rs`
  - `crates/nova-agent/src/agent/runtime.rs`
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`
  - `docs/adr/2026-05-16-orchestrator-agent-task-refactor.md`
  - `docs/2026-05-16-orchestrator-agent-task-refactor/orchestrator-agent-task-refactor.md`
- 允许修改：
  - 与 task 事件映射相关的 UI / app 类型定义
  - 仅测试中需要的辅助构造函数
- 禁止修改：
  - 不要删除 `TaskStore` / `TaskStoreHandle`
  - 不要新增数据库或文件持久化
  - 不要把 `TaskStore` 再次公开为模型可写工具

## Agent 执行步骤

1. 在 `crates/nova-agent/src/tool/builtin/mod.rs` 中移除 `TaskCreate`、`TaskUpdate`、`TaskList` 的默认工具注册
2. 在 `crates/nova-agent/src/tool/builtin/task.rs` 中保留 `TaskStore`、`TaskStoreHandle` 和内部状态更新方法
3. 对 `TaskStore` 增加面向 orchestrator 的辅助接口，禁止继续要求 orchestrator 走工具层完成创建和更新
4. 明确保留 `stage_id` 和 `agent_id` 的内部语义：前者表示执行阶段，后者表示该阶段内的具体子 Agent 实例
5. 若保留 `TaskKeywordDetector` 或相近逻辑，必须评估其是否仍有存在意义；若仅服务旧通用任务入口，则删除
6. 在长期设计资产中回写新的职责边界：`OrchestrateTaskTool` 是唯一对外编排入口，`TaskStore` 是内部状态模型，`AgentExecutor` 是内部执行器
7. 在 ADR 中明确记录为何选择“保留 task，但仅内部使用”的路线 A，而不是完全删除 task
8. 更新总览文档中各 Plan 的完成状态

## 目标数据结构 / 接口契约

```rust
pub struct TaskStoreHandle {
    // 保留会话级状态存储职责
}

impl TaskStoreHandle {
    pub async fn create_orchestration_task(&self, ...) -> Task;
    pub async fn update_orchestration_task(&self, ...) -> Result<Task>;
    pub async fn list_tasks(&self) -> Vec<Task>;
}
```

## 行为规则

| 输入 | 处理路径 | 期望输出或状态变化 |
|------|----------|------------------|
| 模型准备调用 `TaskCreate` | 默认工具集中不存在该工具 | 模型无法直接写 task |
| orchestrator 需要创建子 agent task | 直接调用 `TaskStoreHandle` 内部接口 | task 创建成功并可被 UI / 测试观测 |
| 前端需要展示任务列表 | 读取 session 级 task 状态 | 可看到 orchestrator 创建的内部 task |
| 非编排功能未显式声明需要任务系统 | 不暴露任务工具 | 避免任务系统继续向通用产品能力漂移 |

## 禁止事项

- 不要把 `TaskStore` 完全删除
- 不要保留模型可写的 `TaskCreate` / `TaskUpdate`
- 不要在本 Plan 中额外引入任务持久化、恢复、重试系统
- 不要修改与本次重构无关的普通对话链路

## 测试要求

- 修改或新增 `task.rs` 测试：
  - 验证 orchestrator 内部接口仍能创建、列出、更新 task
  - 验证列表快照语义仍然成立
- 修改或新增工具注册测试：
  - 验证默认工具列表中不再包含 `TaskCreate`、`TaskUpdate`、`TaskList`
  - 验证默认工具列表中不再包含 `Agent`
- 更新长期设计资产与 ADR 后，执行完整修复流程：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [ ] `TaskCreate`、`TaskUpdate`、`TaskList` 不再默认注册为模型工具
- [ ] `TaskStore` 保留并提供内部 orchestration 接口
- [ ] `TaskKeywordDetector` 已删除或明确降级为非默认路径
- [ ] 长期设计资产已同步新职责边界
- [ ] ADR 已记录路线 A 的取舍与影响范围
- [ ] 总览文档中的 Plan 状态已回写
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过

