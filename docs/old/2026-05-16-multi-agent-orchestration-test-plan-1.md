# Plan 1：SubAgentExecutor Trait 重构

- **Plan 编号**：1
- **前置依赖**：无
- **本次目标**：引入 `SubAgentExecutor` trait，使 `OrchestratorEngine` 可注入 mock 实现

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `crates/nova-agent/src/orchestrator/mod.rs` | 主要修改：Engine 持有 trait object |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 新增：为 AgentTool 实现 trait |
| `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` | 适配：构造 Engine 时传入 trait object |

## 详细设计

### 1. Trait 定义

在 `orchestrator/mod.rs` 中定义：

```rust
#[async_trait::async_trait]
pub trait SubAgentExecutor: Send + Sync {
    /// 执行一个 sub-agent，返回工具输出。
    /// input 是传给 AgentTool 的 JSON 参数，context 是 ToolContext。
    async fn execute_agent(
        &self,
        input: serde_json::Value,
        context: Option<ToolContext>,
    ) -> anyhow::Result<ToolOutput>;

    /// 返回 catalog 中已注册的 agent ID 集合。
    fn catalog_agent_ids(&self) -> std::collections::HashSet<String>;

    /// 返回默认 agent ID（用于 fallback）。
    fn default_agent_id(&self) -> String;
}
```

### 2. AgentTool 实现 SubAgentExecutor

在 `tool/builtin/agent.rs` 中：

```rust
#[async_trait::async_trait]
impl SubAgentExecutor for AgentTool {
    async fn execute_agent(
        &self,
        input: serde_json::Value,
        context: Option<ToolContext>,
    ) -> anyhow::Result<ToolOutput> {
        // 委托给现有的 Tool::execute
        self.execute(input, context).await
    }

    fn catalog_agent_ids(&self) -> HashSet<String> {
        self.catalog_agent_ids() // 现有方法
    }

    fn default_agent_id(&self) -> String {
        self.default_agent_id() // 现有方法
    }
}
```

注意：`catalog_agent_ids()` 和 `default_agent_id()` 方法名与 trait 方法重名。如果 Rust 编译器产生歧义，可将 trait 方法改名为 `get_catalog_agent_ids()` / `get_default_agent_id()`，或将现有方法保留并在 trait impl 中直接内联。

### 3. OrchestratorEngine 改造

```rust
pub struct OrchestratorEngine {
    executor: Arc<dyn SubAgentExecutor>,  // 原来是 Arc<AgentTool>
    event_tx: mpsc::Sender<AgentEvent>,
    tool_context: Option<ToolContext>,
    catalog_agent_ids: Arc<HashSet<String>>,
    default_agent_id: String,
}

impl OrchestratorEngine {
    pub fn new(
        executor: Arc<dyn SubAgentExecutor>,  // 原来是 Arc<AgentTool>
        event_tx: mpsc::Sender<AgentEvent>,
        tool_context: Option<ToolContext>,
    ) -> Self {
        let catalog_agent_ids = Arc::new(executor.catalog_agent_ids());
        let default_agent_id = executor.default_agent_id();
        Self { executor, event_tx, tool_context, catalog_agent_ids, default_agent_id }
    }
}
```

`execute_plan` 和 `run_review` 中所有 `self.agent_tool.execute(...)` 替换为 `self.executor.execute_agent(...)`。

### 4. OrchestrateTaskTool 适配

`OrchestrateTaskTool` 构造 `OrchestratorEngine` 时，将 `Arc<AgentTool>` 作为 `Arc<dyn SubAgentExecutor>` 传入——由于 `AgentTool` 实现了 trait，无需 `as` 转换。

## 测试案例

| 测试名 | 场景 | 预期 |
|--------|------|------|
| `existing_clippy_fmt_test_pass` | 重构后 `cargo clippy` + `cargo fmt` + `cargo test` 全通过 | 无回归 |

## 约束

- 不新增外部依赖（`async_trait` 已在 workspace 中）
- 不改变任何外部可观测行为
- `ToolOutput` 类型需从 `tool/registry.rs` re-export 或在 trait 定义处引用
