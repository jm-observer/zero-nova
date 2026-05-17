# Plan 3: 目录扫描与 Deferred 注册

## 前置依赖

Plan 1, Plan 2

## 任务目标

在 agent 启动时扫描配置的 `tools_dir` 目录，将所有外部 tool 注册为真正的 deferred tool（恢复 deferred 语义），
通过 `tool_search` 按需激活。

## 执行范围

- **必须修改**：`crates/nova-agent/src/tool/registry.rs`（恢复 `register_deferred` 真正的 deferred 语义）
- **必须修改**：`crates/nova-agent/src/tool/builtin/mod.rs`（在注册流程中调用外部 tool 加载）
- **必须修改**：`crates/nova-agent/src/tool/mod.rs`（导出 external 模块）
- **允许修改**：`crates/nova-agent/src/tool/external/mod.rs`（新增注册辅助函数）
- **禁止修改**：任何 builtin tool 的行为逻辑

## Agent 执行步骤

### 步骤 1：恢复 `register_deferred` 的真正 deferred 语义

修改 `crates/nova-agent/src/tool/registry.rs`：

将当前的 `register_deferred_with_category` 恢复为将 tool 放入 `state.deferred` 列表：

```rust
pub async fn register_deferred_with_category(
    &self,
    name: String,
    description: String,
    input_schema: Value,
    factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
    category: DeferredToolCategory,
) {
    let entry = DeferredToolEntry {
        name,
        description,
        input_schema,
        factory,
        category,
    };
    let mut state = self.lock_state_async().await;
    state.deferred.push(entry);
    let _ = self.refresh_snapshot_locked_async(&state).await;
}
```

同步修改 `register_deferred` 保持委托关系不变。

### 步骤 2：新增外部 tool 注册辅助函数

在 `crates/nova-agent/src/tool/external/mod.rs` 新增：

```rust
use crate::tool::{DeferredToolCategory, ToolRegistry};
use std::path::Path;
use std::sync::Arc;

/// 扫描目录并将所有外部 tool 注册为 deferred
pub async fn register_external_tools(registry: &ToolRegistry, tools_dir: &Path) {
    let definitions = match load_tools_from_dir(tools_dir) {
        Ok(defs) => defs,
        Err(e) => {
            log::warn!("failed to load external tools from {}: {}", tools_dir.display(), e);
            return;
        }
    };
    log::info!("loaded {} external tool definition(s) from {}", definitions.len(), tools_dir.display());
    for def in definitions {
        let name = def.name.clone();
        let description = def.description.clone();
        let input_schema = def.input_schema.clone();
        let def = Arc::new(def);
        let factory: Box<dyn Fn() -> Arc<dyn crate::tool::Tool> + Send + Sync> = Box::new(move || {
            Arc::new(executor::ExternalCommandTool::from_definition((*def).clone()))
        });
        registry
            .register_deferred_with_category(
                name,
                description,
                input_schema,
                factory,
                DeferredToolCategory::System,
            )
            .await;
    }
}
```

### 步骤 3：在 builtin 注册流程中调用外部 tool 加载

修改 `crates/nova-agent/src/tool/builtin/mod.rs`，在 `register_builtin_tools()` 末尾新增调用点。
或者在 `AgentRuntime` / `AgentApplication` 初始化时，读取 config 后调用 `register_external_tools`。

具体接入点需查看 `AgentRuntime` 的初始化流程确定最佳位置。

### 步骤 4：确认 tool_search 能找到 deferred 外部 tool

验证 `tool_search` 现有逻辑能搜索 `state.deferred` 列表（已有此能力，`resolve_deferred_with_outcome` 会从 deferred 移到 loaded）。

## 行为规则

| 场景 | 处理 | 结果 |
|------|------|------|
| config 未配置 tools_dir | 跳过，不加载任何外部 tool | 行为不变 |
| tools_dir 不存在 | warn 日志，返回空 | 不影响启动 |
| 加载了 N 个外部 tool | 全部进入 deferred 列表 | tool_search 可搜索到 |
| LLM 调用 tool_search("github") | 匹配到 github-commit-info | 返回 schema，激活为 loaded |
| LLM 调用已激活的外部 tool | 执行 ExternalCommandTool | spawn 进程，返回结果 |

## 禁止事项

- 禁止将外部 tool 注册为 always-on（必须走 deferred）
- 禁止修改 `tool_search` 的搜索逻辑（现有逻辑已够用）
- 禁止在加载外部 tool 时 panic（错误仅 warn 跳过）

## 测试要求

- 测试名：`test_register_external_tools_from_dir`
- 输入：创建临时目录，写入测试用 .toml，调用 `register_external_tools`
- 验证：deferred 列表包含注册的 tool，`resolve_deferred` 后可执行
- 验证命令：`cargo test -p nova-agent test_register_external`

## 完成条件

- [ ] `register_deferred_with_category` 恢复真正 deferred 语义
- [ ] 外部 tool 出现在 `deferred_representations` 中
- [ ] `tool_search` 能匹配并激活外部 tool
- [ ] 激活后 `ExternalCommandTool` 可正常执行
- [ ] 不影响现有内置 tool 的注册和使用
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
