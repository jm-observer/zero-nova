# Plan 3：🟡 结构性问题

## Plan 编号与标题

Plan 3：结构性问题（职责过宽、重复定义、空壳文件、参数爆炸等）

## 前置依赖

无（可与 Plan 1/2 并行评审）

## 本次目标

识别并记录当前结构性问题，明确整改方向，为后续 crate 拆分做准备。

补充设计文档：`nova-agent-audit-plan-3-design.md`（覆盖问题 10-16，显式排除问题 9）。

---

## 问题 9：Crate 职责过宽（Monolith）

`nova-agent` 实际上是一个 monolith crate，承载了：

| 子系统 | 模块 |
|--------|------|
| Agent 运行时 | `agent/` |
| LLM Provider 对接 | `provider/`（Anthropic + OpenAI compatible） |
| 工具系统 | `tool/`（12 个 builtin 工具） |
| Prompt 构建 | `prompt/` |
| Skill 路由 | `skill/` |
| 编排器 | `orchestrator/` |
| 会话持久化 | `conversation/`（SQLite 存储） |
| 应用层 | `app/`（Bootstrap、ConversationService、VoiceService） |
| MCP 客户端 | `mcp/` |
| 配置系统 | `config/` |
| 工具函数 | `loop_guard.rs`、`path_resolver.rs`、`network.rs`、`message.rs` |

**问题**：31 个依赖全部捆绑，任何模块改动导致整个 crate 重新编译。会话持久化、Provider 对接、工具实现、配置系统逻辑上可以独立成 crate。

**建议**（长期）：

```
nova-protocol     # 消息格式、事件定义（已有）
nova-provider     # LLM Provider 对接
nova-tools        # 工具系统（builtin 工具 + registry）
nova-conversation # 会话持久化（SQLite）
nova-agent        # 运行时 + 编排器（依赖上述 crate）
```

---

## 问题 10：`ToolDefinition` 重复定义

存在两个同名但不同的 `ToolDefinition`：

- `tool::registry::ToolDefinition` — 含 `defer_loading` 字段，用于工具注册
- `provider::types::ToolDefinition` — 不含 `defer_loading`，用于传给 LLM

两者之间需要手动转换，容易遗漏字段或产生误用。

**建议**：以 `provider::types::ToolDefinition` 为规范定义，`tool::registry` 在注册时通过包装结构携带 `defer_loading` 元数据，避免同名歧义。

---

## 问题 11：空壳 Placeholder 文件

以下文件只有一行注释或一行 re-export，毫无实际内容：

| 文件 | 内容 |
|------|------|
| `agent/stream_bridge.rs` | `// Placeholder module for Plan 4 split.` |
| `agent/turn_executor.rs` | `// Placeholder module for Plan 4 split.` |
| `skill/model.rs` | `pub use super::types::{Skill, SkillPackage, ToolPolicy};` |
| `skill/policy.rs` | `pub use super::types::{CapabilityPolicy, FileToolPriority, PolicySource, ToolStatus};` |
| `conversation/repository/message_repo.rs` | 单行 re-export |
| `conversation/repository/session_repo.rs` | 单行 re-export |

**问题**：

- Plan 4 分拆的占位文件已存在但从未实施，徒增心智负担
- `model.rs` / `policy.rs` 作为纯 re-export 没有拆分价值，只增加间接层
- `message_repo.rs` / `session_repo.rs` 是空壳，实现从未迁入

**建议**：删除占位文件（或立即完成迁移），将 `model.rs` / `policy.rs` 的内容合回 `types.rs`。

---

## 问题 12：`PromptConfig` 参数爆炸

`PromptConfig` 有 **15 个字段** + **12 个 builder 方法**，其中部分字段与 prompt 构建无直接关系：

```rust
pub project_dir: Option<PathBuf>,           // 文件加载配置
pub developer_prompt_files: Vec<String>,    // 文件加载配置
pub project_context_path: Option<PathBuf>,  // 文件加载配置
```

这些实际上是"文件加载上下文"而非"prompt 构建配置"。

**建议**：拆分为 `PromptConfig`（构建参数）和 `PromptLoadContext`（文件路径/预加载内容），由 bootstrap 层负责组装 `PromptLoadContext` 并预加载内容后再传入 `PromptConfig`。

---

## 问题 13：配置模型 Default 函数碎片化

`config/models.rs` 有 **30+ 个独立的 `default_xxx()` 函数**：

```rust
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
fn default_max_iterations() -> usize { 30 }
// ...
```

这些碎片化的函数分散在文件中，难以统一查阅默认值。

**建议**：提取为具名常量（`const DEFAULT_HOST: &str = "127.0.0.1"`），或为每个配置结构体实现 `Default` trait 并集中说明。

---

## 问题 14：`AgentDescriptor` 与 `provider::ModelConfig` 同名

`agent_catalog.rs` 定义了独立的 `ModelConfig`（含 `model`、`temperature`、`max_tokens`、`top_p`），与 `provider::ModelConfig` 完全同名，但用途不同（一个用于 catalog 序列化，一个用于运行时）。

**建议**：将 catalog 内的配置结构重命名为 `AgentModelOverride` 或 `CatalogModelConfig`，消除名称歧义。

---

## 问题 15：`lib.rs` 中的无意义 `run()` 函数

```rust
pub async fn run() -> anyhow::Result<()> {
    log::info!("nova-core started");  // 连 crate 名都写错了
    Ok(())
}
```

这是一个空占位函数，且日志内容写的是 "nova-core" 而非 "nova-agent"。

**建议**：直接删除，无任何调用方依赖此函数。

---

## 问题 16：`#[allow(clippy::too_many_arguments)]` 抑制警告

`run_turn_with_model_config` 和 `run_turn_with_context_and_model_config` 各有 **8 个参数**，通过 `#[allow]` 压制了 clippy 警告。AGENTS.md 明确规定：

> 禁止用 `#[allow(...)]` 压制警告；参数超过 4 个时应引入结构体参数。

**建议**：引入 `TurnParams` 结构体，将 8 个参数收敛到 1 个，并移除 `#[allow]`。

---

## 优先级汇总

| 优先级 | 问题 | 影响 |
|-------|------|------|
| P0 | 问题 1：`prompt/mod.rs` 90KB 巨型文件 | 消除 90KB 单文件，降低维护难度 |
| P0 | 问题 4：同步/异步双写 | 减少约 30% 代码量，消除阻塞 async runtime 隐患 |
| P1 | 问题 8：双 turn 执行路径并存 | 减少分支复杂度 |
| P1 | 问题 6：`CapabilityPolicy` 混入 cache 参数 | 职责分离 |
| P1 | 问题 11：空壳 placeholder 文件 | 消除误导 |
| P1 | 问题 15：无意义 `run()` 函数 | 清理 |
| P1 | 问题 16：`#[allow]` 压制 clippy | 合规 |
| P2 | 问题 2/3：conversation 大文件 | 降低单文件复杂度 |
| P2 | 问题 5：ToolRegistry 双锁 | 降低锁策略复杂度 |
| P2 | 问题 7：AgentEvent 膨胀 | 减少事件通道噪音 |
| P2 | 问题 10：ToolDefinition 重复定义 | 消除转换成本 |
| P2 | 问题 14：ModelConfig 同名歧义 | 消除混淆 |
| P3 | 问题 9：Crate monolith | 改善编译时间（长期规划） |
| P3 | 问题 6：Skill / SkillPackage 共存 | 简化模型 |
| P3 | 问题 12：PromptConfig 参数爆炸 | 降低调用方复杂度 |
| P3 | 问题 13：配置 default 碎片化 | 改善可读性 |
