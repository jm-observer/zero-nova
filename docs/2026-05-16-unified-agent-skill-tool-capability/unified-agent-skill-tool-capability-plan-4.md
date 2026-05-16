# Plan 4: 移除 Deferred Tool 设计

## 前置依赖

- Plan 1: 统一 Tool 注册模型
- Plan 2: 删除 Turn 级 Tool 裁剪
- Plan 3: 收敛 Prompt / Skill 行为语义

## 任务目标

删除 ToolRegistry 中的 deferred tool 设计，使所有工具在运行时启动后即作为普通 loaded tool 可见和可调用。

完成后应满足：

- `Skill`、`TaskCreate`、`TaskList`、`TaskUpdate` 直接出现在当前工具列表中
- `ToolSearch` 不再承担“加载 deferred tool”的入口职责
- `tool_definitions()` 直接返回完整统一工具集，不再注入 ToolSearch stub
- ToolRegistry 不再维护 loaded/deferred 双态模型

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/tool/registry.rs`
  - `crates/nova-agent/src/tool/mod.rs`
  - `crates/nova-agent/src/tool/builtin/mod.rs`
  - `crates/nova-agent/src/tool/builtin/tool_search.rs`
  - `crates/nova-cli/src/main.rs`
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`
  - `docs/adr/2026-05-16-unified-agent-skill-tool-capability.md`
- 允许修改：
  - 与 ToolSearch / deferred tool 相关测试
  - `.nova/prompts/agent-*.md` 中对 ToolSearch 的引用文案
- 禁止修改：
  - 不要重新引入新的延迟加载替代层
  - 不要把 deferred 语义隐藏为另一个近似概念

## Agent 执行步骤

1. 在 `ToolRegistry` 中删除 deferred 状态、表示结构和 resolve/load/filter 相关接口
2. 删除 `tool_definitions()` 中注入 ToolSearch stub 的逻辑
3. 将 `register_deferred*()` 的调用点改为直接实例化并注册真实工具，至少包括 `Skill`、`TaskCreate`、`TaskList`、`TaskUpdate`
4. 从 builtin 入口移除 `ToolSearch` 的注册和执行分发
5. 收敛 CLI、tool metadata、测试中的 loaded/deferred 双态观测逻辑
6. 更新设计文档与 ADR，声明工具系统已从双态模型收敛为单态 loaded 模型

## 目标数据结构 / 接口契约

目标 `ToolRegistry` 方向：

```rust
pub struct ToolRegistry {
    state: Mutex<RegistryState>,
    snapshot: RwLock<Arc<RegistrySnapshot>>,
}

struct RegistryState {
    tools: Vec<Arc<dyn Tool>>,
}
```

目标 `tool_definitions()` 语义：

```rust
pub async fn tool_definitions(&self) -> Vec<ProviderToolDefinition>
// 返回全部已注册工具；不再附加 ToolSearch 入口
```

## 行为规则

| 输入 / 场景 | 期望结果 |
|------|----------|
| runtime 启动 | `Skill`、`Task*` 等工具已直接注册 |
| prompt 构建 | `Tool Capabilities` 中直接出现全部工具 |
| 模型需要使用 Skill | 直接调用 `Skill`，不需要 ToolSearch |
| CLI 查看工具 | 不再区分 loaded / deferred 数量 |

## 禁止事项

- 不要保留死代码形式的 Deferred 类型和枚举
- 不要仅修改 prompt 文案而保留 registry 双态实现
- 不要让 `ToolSearch` 继续作为“发现工具”的唯一入口

## 测试要求

- 新增或修改测试，覆盖：
  - `tool_definitions()` 直接返回 `Skill` / `Task*`
  - `ToolSearch` 不再作为加载入口存在，或其行为被显式收敛
  - `ToolInfo` 能直接查询 `Skill` / `Task*`
- 必须执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [x] ToolRegistry 不再维护 deferred tool 双态
- [x] `Skill` 与 `Task*` 已作为普通 loaded tool 暴露
- [x] `ToolSearch` 的延迟加载职责已删除
- [x] 工具元数据与 CLI 输出已收敛为单态模型
- [x] 设计文档与 ADR 已同步更新
- [x] `cargo clippy --workspace -- -D warnings` 通过
- [x] `cargo fmt --check --all` 通过
- [x] `cargo test --workspace` 通过
