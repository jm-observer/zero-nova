# Plan 2: 删除 Turn 级 Tool 裁剪

## 前置依赖

- Plan 1: 统一 Tool 注册模型

## 任务目标

删除 active skill 对当前轮工具可见性的裁剪逻辑，使 skill 只影响 prompt / metadata，不再影响工具集合。

完成后应满足：

- `prepare_turn()` 不再基于 active skill retain 工具定义
- `CapabilityPolicy` 不再承担工具开关职责
- active skill 命中后，模型仍看到完整统一工具集

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/agent/runtime.rs`
  - `crates/nova-agent/src/skill/types.rs`
  - `crates/nova-agent/src/skill/registry/filter.rs`
- 允许修改：
  - 与 capability policy / tool visibility 相关测试
- 禁止修改：
  - agent prompt 文案
  - tool 注册逻辑
  - orchestrator 业务逻辑

## Agent 执行步骤

1. 在 `crates/nova-agent/src/agent/runtime.rs` 中移除 `prepare_turn()` 内基于 active skill 的工具 retain 逻辑
2. 删除或简化 `filter_tool_definitions()`，改为直接返回统一工具定义集合
3. 在 `crates/nova-agent/src/skill/registry/filter.rs` 中删除 `policy_from_skill()` 对工具开关的职责，保留 skill 匹配与必要元数据能力
4. 在 `crates/nova-agent/src/skill/types.rs` 中收敛 `CapabilityPolicy`，移除与工具白名单 / deferred tool 相关的语义
5. 修改测试，验证命中 active skill 前后，本轮可见工具集合保持一致

## 目标数据结构 / 接口契约

目标 `prepare_turn()` 语义：

```rust
// active skill may affect prompt context, but not visible tool definitions
pub async fn prepare_turn(...) -> Result<TurnContext>
```

目标 `CapabilityPolicy` 方向：

```rust
pub struct CapabilityPolicy {
    pub source: PolicySource,
    pub file_tool_priority: FileToolPriority,
}
```

若实施中发现 `CapabilityPolicy` 可整体删除，可在本 Plan 内一并收敛，但必须同步所有调用点。

## 行为规则

| 输入 / 场景 | 期望结果 |
|------|----------|
| 普通输入，无 active skill | 显示完整统一工具集 |
| `/orchestrator ...` 命中 active skill | 仍显示完整统一工具集 |
| active skill = `allow_list` | 不再裁剪工具，仅保留 skill 提示语义 |
| `ToolInfo` 查询某工具 | 结果只受统一工具注册影响，不受 active skill 影响 |

## 禁止事项

- 不要在本 Plan 中重写 skill prompt 注入模式
- 不要在本 Plan 中新增新的工具访问控制层
- 不要顺手删除 `enable_project_developer_prompt`

## 测试要求

- 新增或修改测试，覆盖：
  - active skill 前后 `visible_tool_names` 相同
  - `ToolInfo` 在 active skill 场景下仍能看到统一工具集合
- 必须执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [ ] turn 级 skill tool filtering 已移除
- [ ] active skill 不再改变本轮工具集合
- [ ] `CapabilityPolicy` 已收敛或删除工具开关职责
- [ ] 测试覆盖 active skill 前后工具可见性一致
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过

