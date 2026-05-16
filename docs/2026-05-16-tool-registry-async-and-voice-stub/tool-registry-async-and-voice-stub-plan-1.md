# Plan 1: 统一 ToolRegistry 为异步公共接口

## 前置依赖

无

## 任务目标

删除 `ToolRegistry` 对外暴露的同步注册与读取接口，只保留异步公共方法，并完成所有调用面的同步迁移。完成后：

- `register`、`register_many`、`register_deferred`、`register_deferred_with_category` 改为异步公共接口。
- `tool_definitions`、`loaded_definitions`、`deferred_definitions_snapshot`、`get_turn_view`、`filter_deferred_by_policy`、`deferred_tools_by_category` 不再作为同步公共接口存在。
- `lock_state_sync`、`lock_snapshot_sync`、`refresh_snapshot_locked_sync` 及其自旋逻辑删除。

## 执行范围

| 类别 | 路径 | 说明 |
| --- | --- | --- |
| 必须修改 | `crates/nova-agent/src/tool/registry.rs` | 删除同步公共 API 与同步锁辅助函数 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/mod.rs` | 改为异步注册 built-in tools |
| 必须修改 | `crates/nova-agent/src/agent/runtime.rs` | 适配异步 registry 调用 |
| 必须修改 | `crates/nova-agent/src/prompt/builder.rs` | 适配异步读取工具定义 |
| 必须修改 | `crates/nova-cli/src/main.rs` | 适配异步 CLI 读取 |
| 允许修改 | `crates/nova-agent/tests/**` | 调整测试为异步调用 |
| 禁止修改 | `crates/nova-agent/src/tool/schema_validation.rs` | 不修改 schema 校验语义 |
| 禁止修改 | `crates/nova-agent/src/tool/path_preprocess.rs` | 不修改文件路径预处理语义 |

## Agent 执行步骤

1. 在 `ToolRegistry` 中删除同步公共读取/注册方法及其同步锁辅助函数。
2. 将保留的异步方法统一命名为主接口；若当前存在 `*_async` 后缀，必须收敛命名，避免同时保留两套公共 API。
3. 修改 `register_builtin_tools` / `register_builtin_tools_with_services` 为异步函数，并顺序调用异步注册接口。
4. 修改运行时和 loader 装配路径，确保构造 `ToolRegistry` 后在异步上下文中完成注册。
5. 调整 prompt 构建、工具信息查询、CLI 输出和测试代码，统一使用异步读取接口。
6. 保留 `ToolRegistry::execute`、deferred tool 解析与 schema 校验现有行为，不得改变用户可见工具语义。

## 目标数据结构 / 接口契约

```rust
impl ToolRegistry {
    pub async fn register(&self, tool: Box<dyn Tool>);
    pub async fn register_many(&self, tools: Vec<Box<dyn Tool>>);
    pub async fn register_deferred(...);
    pub async fn register_deferred_with_category(...);

    pub async fn tool_definitions(&self) -> Vec<ProviderToolDefinition>;
    pub async fn loaded_definitions(&self) -> Vec<RegisteredToolDefinition>;
    pub async fn deferred_definitions(&self) -> Vec<RegisteredToolDefinition>;
    pub async fn get_turn_view(...) -> TurnToolView;
}
```

## 行为规则

| 输入 / 场景 | 处理路径 | 期望结果 |
| --- | --- | --- |
| 启动时注册 built-in tools | 异步顺序注册 | 工具集合与当前保持一致 |
| prompt 构建读取工具定义 | 异步读取 snapshot | 构建结果保持一致 |
| CLI 列出工具 | 异步读取 loaded / deferred definitions | 输出格式保持一致 |
| 测试中注册工具 | `await` 注册接口 | 行为保持一致 |

## 禁止事项

- 不要保留同步公共方法作为兼容层。
- 不要在异步方法外层再包一层新的同步自旋。
- 不要修改 deferred tool 的加载时机与类别规则。
- 不要新增依赖。

## 测试要求

| 测试文件 | 测试名称 | 输入 | 期望断言 |
| --- | --- | --- | --- |
| `crates/nova-agent/src/tool/registry.rs` | 现有 registry 相关测试 | 现有输入 | 全部通过并改为异步调用新接口 |
| `crates/nova-agent/src/tool/builtin/mod.rs` | 现有 built-in tool 注册测试 | 白名单/默认场景 | 继续通过 |
| `crates/nova-cli/src/main.rs` | CLI 编译路径 | `--list-tools` 相关读取 | 编译通过 |

必须执行的验证命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `ToolRegistry` 同步公共 API 已删除
- [ ] 同步自旋锁辅助函数已删除
- [ ] built-in tool 注册改为异步
- [ ] `nova-agent`、`nova-cli`、测试调用面已完成迁移
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
