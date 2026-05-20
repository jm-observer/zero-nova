# Skill / Tool 只读 Listing API（供 zero Web 控制台目录视图调用）

## 时间

- 创建时间：2026-05-20
- 最后更新：2026-05-20（设计稿，待评审与实施）

## 背景与触发方

下游消费方为 zero 仓库的 Web 管理控制台「目录视图」（设计稿见 zero 仓 `docs/2026-05-20-console-catalog-view/console-catalog-view.md`）。该视图需在浏览器只读展示：

- 全部 SKILL 的 `slug` / `display_name` / `description` / `source_path` / `preload`
- 全部已加载（loaded / always-on）工具的 `name` / `description` / `input_schema`
- 全部 deferred 工具的 `name` / `description` / `input_schema` / `category`

zero 是 nova-agent 的 git tag 依赖（`zero/Cargo.toml`：`nova-agent = { git = "...zero-nova.git", tag = "v0.3.3" }`），消费方无法穿透 nova-agent 内部抓取——必须经 **nova-agent 对外 API**。本设计落地这层最小 listing API。

> 一旦本设计实施完成、发出新 nova tag（如 v0.3.4），zero 侧才能继续推进其 console-catalog-view 的 Plan 1。

## 项目现状（代码勘察）

| 关注点 | 现状 | 证据 |
|--------|------|------|
| `SkillRegistry` | `packages: Vec<SkillPackage>` 字段 pub，`SkillPackage` 全字段 pub（含 `slug` / `description` / `source_path` / `preload`） | `crates/nova-agent/src/skill/registry.rs:11`、`crates/nova-agent/src/skill/types.rs:60` |
| `SkillRegistry` 归属 | `AgentWorkspaceService.skill_registry: Arc<SkillRegistry>`（pub 字段，归属于 `AgentApplicationImpl.workspace_service`） | `crates/nova-agent/src/app/agent_workspace_service.rs:51` |
| `ToolRegistry` 归属 | `ConversationService.agent: AgentRuntime`（pub），`AgentRuntime.tools: ToolRegistry`（私有，但有 `pub fn tools(&self) -> &ToolRegistry`） | `crates/nova-agent/src/app/conversation_service.rs:101`、`crates/nova-agent/src/agent/runtime.rs:140` |
| `ToolRegistry` 内部状态 | `state: Mutex<RegistryState>` + `snapshot: RwLock<Arc<RegistrySnapshot>>`；`RegistrySnapshot` 私有，含 `loaded_definitions` / `deferred_definitions` / `deferred_representations` | `crates/nova-agent/src/tool/registry.rs:97-138` |
| `AgentApplicationImpl` 对外 API | 仅 `list_agents` / `get_agent` 等 agent 维度方法，**无 skill / tool listing** | `crates/nova-agent/src/app/application.rs:316` 等 |
| 公开类型已就绪 | `SkillPackage`、`RegisteredToolDefinition`、`DeferredToolRepresentation`、`DeferredToolCategory` 均已 pub export 自 `lib.rs` | `crates/nova-agent/src/lib.rs:33-34` |

**结论**：数据全在内存里、相关类型已 pub，只缺 `AgentApplicationImpl` 对外 getter 和 `ToolRegistry` 的 snapshot listing 公开方法。本次扩展是**纯 add-only**，不改任何既有行为、不破坏既有 API。

## 整体目标

在 `AgentApplicationImpl` 上新增两个只读方法，配套在 `ToolRegistry` 上新增最小 listing 公开方法，使外部消费方（zero 控制台）可以一次拉到 skills/tools 全量元数据：

```rust
impl AgentApplicationImpl {
    pub fn list_skills(&self) -> Vec<SkillPackage>;
    pub fn list_tools(&self) -> ToolInventoryView;
}

pub struct ToolInventoryView {
    pub loaded: Vec<RegisteredToolDefinition>,
    pub deferred: Vec<DeferredToolRepresentation>,
}
```

**核心取舍（最小改动）**：

- **返回克隆数据而非引用**：与 `AgentApplicationImpl::list_agents()` 现有风格一致（`-> Vec<AppAgent>`），调用方拿到的是独立快照、无生命周期问题、无 Mutex/RwLock 暴露
- **复用既有 pub 类型**：`SkillPackage`、`RegisteredToolDefinition`、`DeferredToolRepresentation` 已 pub；新增 `ToolInventoryView` 作为聚合包装（小、无逻辑、纯数据）
- **不暴露 `&SkillRegistry` / `&ToolRegistry` 引用**：保留未来重构内部结构的自由度
- **不动 `RegistrySnapshot` 私有性**：在 `ToolRegistry` 上加两个公开 listing 方法，内部克隆 snapshot 字段即可

> 否决备选：
> - **方案 X：直接 `pub fn skill_registry(&self) -> Arc<SkillRegistry>`** —— 否决，暴露内部容器、调用方可拿引用做意料之外的事；亦让未来 SkillRegistry 重构泄漏到外部
> - **方案 Y：把 listing 做成 protocol 层 RPC 接口** —— 否决，超出本任务范围（消费方在同进程，无需 RPC），且 nova-server-ws 协议变更影响面大
> - **方案 Z：把 `RegistrySnapshot` 全 pub** —— 否决，会把内部表示固化为公开契约，妨碍后续优化

## 配套数据结构（已就绪 / 新增）

| 类型 | 状态 | 来源 |
|------|------|------|
| `SkillPackage` | 已 pub export | `lib.rs:33` |
| `RegisteredToolDefinition` | 已 pub export | `lib.rs:34` |
| `DeferredToolRepresentation` | 已 pub（模块层），需 export 到 `lib.rs` 顶层 | `tool/registry.rs:233` |
| `DeferredToolCategory` | 已 pub（模块层），需 export 到 `lib.rs` 顶层 | `tool/registry.rs:203` |
| `ToolInventoryView` | **新增** | 本设计 |

## Plan 拆分

| 顺序 | Plan | 依赖 | 状态 |
|------|------|------|------|
| 1 | [新增 listing API + 配套 pub export + 测试](skill-tool-listing-api-plan-1.md) | 无 | ⏳ 待评审 |

单 Plan，工作量小、聚焦。完成后走 CLAUDE.md 修复流程（clippy + fmt + test 全绿）+ 升 patch 版本（`v0.3.3` → `v0.3.4`）+ 推 tag。

## 需要更新的 docs/design 与设计影响记录

- 待实施完成后更新 `docs/design/nova-agent-engine-boundaries.md`：在「对外 API」段补一条 listing API 行
- 新增 `docs/adr/2026-05-20-skill-tool-listing-api.md`：记录方案 X/Y/Z 的否决理由（按本总览「核心取舍」段落）
- 标注关联：下游 zero 仓 `docs/2026-05-20-console-catalog-view/console-catalog-view-plan-1.md`

## 风险与待定项

- **版本协调**：本设计要求发新 nova tag（`v0.3.4`）；zero 仓须随后同步改 `Cargo.toml` 的 `nova-agent` / `nova-agent-loader` tag。两侧版本耦合属于已知流程（zero 侧记忆 `project_zero_nova_custom_utils_coupling`），按 patch 升、保持向后兼容（本次纯 add-only，符合 patch 升级语义）。
- **`ToolRegistry` 锁开销**：`list_loaded_definitions` / `list_deferred_representations` 读 `snapshot: RwLock<Arc<RegistrySnapshot>>` 后克隆字段。RwLock 读锁极短、克隆 `Vec<RegisteredToolDefinition>` O(n)，控制台周期（zero 侧默认 3s）调用一次，开销可忽略。
- **deferred 工具的 input_schema 体积**：`RegisteredToolDefinition.input_schema: Value` 单个可能上 KB；当前 deferred 工具数量级 < 50，总响应 < 100 KB，可接受。如未来量级膨胀再考虑 schema 折叠。
- **不破坏既有路径**：新增方法不动 `AgentApplicationImpl` 既有签名、不改 `ToolRegistry` 内部状态结构、不改 `SkillRegistry` 公开字段。`cargo test --workspace` 既有用例应零回归。
- **跨平台**：纯 Rust、无平台分支，windows + linux 双目标编译走修复流程验证。
