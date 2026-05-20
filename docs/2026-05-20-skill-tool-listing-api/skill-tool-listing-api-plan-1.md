# Plan 1: 新增 listing API + 配套 pub export + 测试

## 前置依赖

无。

## 任务目标

在 `nova_agent::app::AgentApplicationImpl` 上新增两个只读 listing 方法，配套在 `ToolRegistry` 上加最小公开 listing 方法、在 `lib.rs` 顶层 export `DeferredToolRepresentation` / `DeferredToolCategory` / 新类型 `ToolInventoryView`。

完成后，下游消费方（zero）能通过：

```rust
let app: &AgentApplicationImpl = /* ... */;
let skills: Vec<SkillPackage> = app.list_skills();
let tools: ToolInventoryView = app.list_tools();
```

拿到 skills/tools 全量元数据，且不接触 nova-agent 内部 registry 引用。

## 执行范围

**必须修改**：

| 文件 | 改动 |
|------|------|
| `crates/nova-agent/src/tool/registry.rs` | 在 `impl ToolRegistry` 上新增 `pub fn list_loaded_definitions(&self) -> Vec<RegisteredToolDefinition>` 与 `pub fn list_deferred_representations(&self) -> Vec<DeferredToolRepresentation>` |
| `crates/nova-agent/src/app/application.rs` | 在 `impl AgentApplicationImpl` 上新增 `pub fn list_skills(&self) -> Vec<SkillPackage>` 与 `pub fn list_tools(&self) -> ToolInventoryView` |
| `crates/nova-agent/src/app/mod.rs`（或就近的 app 出口） | 新增 `ToolInventoryView` 类型定义（小、纯数据），并从 `nova_agent::app` 模块 pub export |
| `crates/nova-agent/src/lib.rs` | 顶层 `pub use` 补充 `DeferredToolRepresentation`、`DeferredToolCategory`、`ToolInventoryView` |

**允许修改**：

- `Cargo.toml`（根）：版本号从 `0.3.3` 升到 `0.3.4`
- `crates/nova-agent/Cargo.toml`：若版本随 workspace；按现有版本管理方式处理

**禁止修改**：

- 任何 `RegistryState` / `RegistrySnapshot` 私有字段的可见性
- `AgentRuntime` / `SkillRegistry` 既有公开签名（仅可新增、不可改）
- 任何会改变运行时行为的代码（本设计纯 add-only）
- 配置文件 / prompt 文件 / agent runtime 行为

## Agent 执行步骤

1. **新增 `ToolInventoryView` 类型**（位置建议 `app/types.rs` 或新建 `app/inventory.rs`，与 `AppAgent` 风格一致）：

   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct ToolInventoryView {
       pub loaded: Vec<RegisteredToolDefinition>,
       pub deferred: Vec<DeferredToolRepresentation>,
   }
   ```

2. **扩 `ToolRegistry`**（`crates/nova-agent/src/tool/registry.rs` 的 `impl ToolRegistry` 末尾）：

   ```rust
   pub fn list_loaded_definitions(&self) -> Vec<RegisteredToolDefinition> {
       self.snapshot.read().expect("snapshot lock poisoned").loaded_definitions.clone()
   }

   pub fn list_deferred_representations(&self) -> Vec<DeferredToolRepresentation> {
       self.snapshot.read().expect("snapshot lock poisoned").deferred_representations.clone()
   }
   ```

   两方法均克隆 snapshot 字段返回，调用方无锁、无引用借用。

3. **扩 `AgentApplicationImpl`**（`crates/nova-agent/src/app/application.rs` 的 `impl AgentApplicationImpl` 末尾、靠近 `list_agents` 处）：

   ```rust
   pub fn list_skills(&self) -> Vec<SkillPackage> {
       self.workspace_service.skill_registry.packages.clone()
   }

   pub fn list_tools(&self) -> ToolInventoryView {
       let tools = self.conversation_service.agent.tools();
       ToolInventoryView {
           loaded: tools.list_loaded_definitions(),
           deferred: tools.list_deferred_representations(),
       }
   }
   ```

4. **顶层 export**（`crates/nova-agent/src/lib.rs`）：

   - 在现有 `pub use tool::{...}` 处补 `DeferredToolRepresentation`、`DeferredToolCategory`
   - 新增 `pub use app::ToolInventoryView`（路径按实际放置位置）

5. **版本号**：根 `Cargo.toml` workspace.package.version 从 `0.3.3` 升 `0.3.4`（若版本管理在 nova-agent 自己 crate 处，相应处理）

6. **修复流程**：`cargo clippy --workspace -- -D warnings` + `cargo fmt --check --all` + `cargo test --workspace`

7. **发版**：commit + 推 `v0.3.4` tag（按 zero-nova 现有发版流程）

## 目标数据结构 / 接口契约

```rust
// nova_agent::app
pub struct ToolInventoryView {
    pub loaded: Vec<RegisteredToolDefinition>,
    pub deferred: Vec<DeferredToolRepresentation>,
}

// impl AgentApplicationImpl
pub fn list_skills(&self) -> Vec<SkillPackage>;
pub fn list_tools(&self) -> ToolInventoryView;

// impl ToolRegistry
pub fn list_loaded_definitions(&self) -> Vec<RegisteredToolDefinition>;
pub fn list_deferred_representations(&self) -> Vec<DeferredToolRepresentation>;
```

`lib.rs` 新增顶层 export：

```rust
pub use tool::{DeferredToolRepresentation, DeferredToolCategory};
pub use app::ToolInventoryView;
```

## 行为规则

| 场景 | 行为 |
|------|------|
| 无任何 SKILL 加载 | `list_skills()` 返回空 vec |
| 无任何 always-on 工具 | `list_tools().loaded` 为空 |
| 无任何 deferred 工具 | `list_tools().deferred` 为空 |
| 同一 deferred 工具被某 session 激活 | 仍出现在 `deferred`（snapshot 表示**注册态**，不反映 session 激活态——后者属于运行时 per-session 隔离，不在本 API 职责） |
| 调用者并发调用 listing | RwLock 读锁；并发安全；返回值彼此独立克隆 |
| snapshot RwLock 中毒 | 当前选择 `expect`+`panic`（与 nova-agent 既有锁失败处理风格一致；如代码库已统一改 `try_read` 或返回 Result，对齐既有风格） |

## 禁止事项

- 不引入任何新外部依赖
- 不在 listing 路径产生任何 I/O（扫盘 / 网络 / 文件读取）
- 不暴露 `&SkillRegistry` / `&ToolRegistry` / `Arc<SkillRegistry>` 给外部
- 不修改 `SkillPackage` / `RegisteredToolDefinition` / `DeferredToolRepresentation` 字段
- 不改 `RegistrySnapshot` 私有性

## 测试要求

| 文件 | 测试名 | 输入 | 断言 |
|------|--------|------|------|
| `crates/nova-agent/src/tool/registry.rs` 内 mod tests | `list_loaded_definitions_returns_registered_tools` | 注册 2 个 always-on 工具的 ToolRegistry | 返回 2 项，name 正确 |
| 同上 | `list_loaded_definitions_empty_when_no_tools` | 默认 ToolRegistry | 返回空 vec |
| 同上 | `list_deferred_representations_returns_registered_deferred` | 注册 2 个 deferred 工厂 | 返回 2 项，name/category 正确 |
| 同上 | `list_deferred_representations_unaffected_by_session_activation` | 注册 1 个 deferred；在 session "s1" 激活 | `list_deferred_representations()` 仍返回 1 项（激活与否不影响注册态视图） |
| `crates/nova-agent/src/app/application.rs` 内 mod tests（或就近集成测） | `list_skills_returns_registered_packages` | 构造 AgentApplicationImpl，注入含 2 个 SkillPackage 的 SkillRegistry | 返回 2 项，slug 正确 |
| 同上 | `list_tools_aggregates_loaded_and_deferred` | 构造含 1 loaded + 1 deferred 的 AgentApplicationImpl | `ToolInventoryView.loaded.len()==1` 且 `deferred.len()==1` |
| 同上 | `list_skills_returns_empty_when_no_skills` | 空 SkillRegistry | 返回空 vec |

> `AgentApplicationImpl` 集成测的 fixture 搭建若过重，可拆出 helper（参考既有 `list_agents` 的测试搭建路径）。如确实重得不偿失，可在 `AgentApplicationImpl` 内方法上加 `cfg(test)` mod 直接走 `workspace_service.skill_registry.clone()` 的等价单测，并在 PR 描述说明范围取舍。

验证命令：

```
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] 4 个新方法 + 1 个新类型编译通过
- [ ] `lib.rs` 新增 export 可被外部 crate 解析
- [ ] 全部新增测试通过
- [ ] 修复流程三步全绿
- [ ] 版本号升至 `v0.3.4`
- [ ] commit message 标注 `feat(app): add list_skills / list_tools introspection API`
- [ ] 推 `v0.3.4` tag
- [ ] 通知 zero 仓更新 `Cargo.toml` 引用（zero 仓 console-catalog-view Plan 1 解除阻塞）
