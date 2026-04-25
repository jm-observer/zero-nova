# Prompt 架构增强实现审查报告

- 日期：2026-04-26
- 状态：审查完成
- 范围：Phase 1 / Phase 2 / Phase 3 全部设计目标的实现状态
- 审查基线：`docs/2026-04-25-prompt-architecture-enhancement.md` 及三个 Phase 子文档

---

## 一、总体结论

三个 Phase 的设计目标 **已全部实现代码**，核心架构改造（统一 prompt 构建管道、环境快照、Skill 按需注入、模板替换、历史裁剪、侧信道注入、prepare_turn 接入）均已落地到对应文件。

但审查中发现 **14 个问题**，按严重程度分为：

| 等级 | 数量 | 说明 |
|------|------|------|
| 🔴 高 | 3 | 功能缺陷或逻辑错误，影响运行时行为 |
| 🟡 中 | 6 | 设计偏差或遗漏，不影响编译但影响预期行为 |
| 🟢 低 | 5 | 代码质量、一致性、测试覆盖不足 |

---

## 二、逐项审查

### Phase 1：统一与修复

#### ✅ G1 — 统一 Prompt 构建管道

**状态：已实现**

- `PromptConfig` 结构体：`prompt.rs:37-83` ✅
- `SystemPromptBuilder::from_config()`：`prompt.rs:543-576` ✅
- `bootstrap.rs` 使用 `from_config()`：`bootstrap.rs:75-78` ✅
- `agent.rs build_system_prompt()` 使用 `from_config()`：`agent.rs:641-648` ✅
- `TemplateContext`：`prompt.rs:107-145` ✅

**问题：无**

---

#### ✅ G2 — 修复 build() 过滤逻辑

**状态：已实现**

- `build()` 方法：`prompt.rs:581-588`
- 仅使用 `.filter(|(_, section)| !section.content.is_empty())` ✅
- 原设计文档中的恒真式 bug 已修复 ✅

**问题：无**

---

#### ✅ G3 — 修复 AllowList 工具消失

**状态：已实现**

- `policy_from_skill()`：`skill.rs:601-641`
- 白名单中的基础工具保留在 `always_enabled_tools` ✅
- 非基础工具放入 `deferred_tools` ✅
- `AllowListWithDeferred` 保留 `tool_search_enabled` ✅
- 测试覆盖：`policy_from_skill_allow_list_preserves_base_tools`、`policy_from_skill_allow_list_empty_keeps_no_base_tools` ✅

**问题：无**

---

#### ✅ G4 — 结构化 Section 输出

**状态：已实现**

- `SectionName::heading()` 方法：`prompt.rs:304-319` ✅
- `build()` 使用 `## heading` + `---` 分隔：`prompt.rs:585` ✅

**问题：无**

---

### Phase 2：核心增强

#### ✅ G5 — 环境快照注入

**状态：已实现**

- `EnvironmentSnapshot` 结构体：`prompt.rs:148-166` ✅
- `collect()` 异步方法：`prompt.rs:173-209` ✅
- `run_git()` 失败静默跳过：`prompt.rs:213-232` ✅
- `to_prompt_text()`：`prompt.rs:235-259` ✅
- `environment_snapshot()` builder 方法：`prompt.rs:502-504` ✅
- `bootstrap.rs` 采集并传入：`bootstrap.rs:33-38, 76` ✅
- `PromptConfig.environment` 字段：`prompt.rs:49` ✅
- `from_config()` 注入环境快照：`prompt.rs:571-573` ✅

**🟡 R1：环境快照缺少测试覆盖**

`EnvironmentSnapshot::to_prompt_text()` 没有对应的单元测试。设计文档 Phase 2 §8.1 列出了 `env_snapshot_to_prompt_includes_cwd`、`env_snapshot_to_prompt_optional_git`、`env_snapshot_to_prompt_with_commits` 三个测试，但代码中均未实现。

---

#### ✅ G6 — 项目上下文加载

**状态：已实现**

- `load_project_context()`：`prompt.rs:266-288` ✅
- `PROJECT_CONTEXT_FILES` 常量：`prompt.rs:17` ✅
- `MAX_PROJECT_CONTEXT_CHARS` 截断：`prompt.rs:272-278` ✅
- `from_config()` 中调用：`prompt.rs:566-568` ✅

**🟡 R2：`load_project_context` 使用 `std::fs::read_to_string`（同步 I/O）**

`prompt.rs:269` 中调用 `std::fs::read_to_string` 在 async 上下文中执行同步文件读取。根据项目代码规范（CLAUDE.md / AGENTS.md）："Never call blocking APIs (`std::fs`, `std::thread::sleep`, sync I/O) in async contexts — use tokio equivalents or `spawn_blocking`"。

`from_config()` 本身是同步方法，但它会在 `bootstrap.rs` 的 async 函数中被调用，间接违反了规范。不过因为 `PROJECT.md` 通常很小，实际影响有限。

**建议**：将 `load_project_context` 改为 `async fn` 使用 `tokio::fs::read_to_string`，或在调用处使用 `spawn_blocking`。也可以接受当前方案并添加注释说明原因。

**🟡 R3：缺少 `config.toml` 中的 `project_context_file` 自定义路径支持**

设计文档 Phase 2 §3.3 定义了 `ToolConfig.project_context_file` 字段和 `load_project_context_with_config()` 函数。但当前实现：
- `config.rs` 中的 `ToolConfig` **没有** `project_context_file` 字段
- `load_project_context()` 不接受自定义路径参数

这是设计文档标注为"可选"的功能，当前缺失不影响核心流程，但与设计文档不一致。

**🟢 R4：`load_project_context` 缺少测试**

设计文档列出了 `load_project_context_finds_file`、`load_project_context_none_when_missing`、`load_project_context_skips_empty`、`load_project_context_truncates_large` 四个测试，均未实现。

---

#### ✅ G7 — Skill 按需注入

**状态：已实现**

- `generate_contextual_prompt()`：`skill.rs:507-553` ✅
- `generate_system_prompt()` 标记 `#[deprecated]`：`skill.rs:558` ✅
- `from_config()` 使用 `generate_contextual_prompt()`：`prompt.rs:560` ✅
- 测试覆盖：`contextual_prompt_no_active_shows_index`、`contextual_prompt_with_active_shows_full`、`contextual_prompt_empty_registry` ✅

**🔴 R5：`from_config()` 使用已标注 `#[deprecated]` 的方法时会产生编译警告**

虽然 `from_config()` 已改用 `generate_contextual_prompt()`，但 `generate_system_prompt()` 标记为 `#[deprecated]` 后，如果项目中其他地方仍有调用（需确认），会产生编译警告。当前代码中 `from_config()` 正确使用了新方法，此项为提醒。

实际上不是 bug，但需要确认全项目不存在对 `generate_system_prompt()` 的未迁移调用。

---

#### ✅ G8 — 模板变量替换增强

**状态：已实现**

- `TEMPLATE_VAR_RE` 正则：`prompt.rs:86` ✅
- `TemplateContext::render()` 清理模式：`prompt.rs:114-121` ✅
- `TemplateContext::render_partial()` 保留模式：`prompt.rs:126-136` ✅
- `TemplateContext::extract_vars()`：`prompt.rs:139-144` ✅
- `template_vars` 常量模块：`prompt.rs:89-104` ✅
- `bootstrap.rs` 传入默认模板变量：`bootstrap.rs:70-73` ✅
- 测试覆盖：4 个测试 ✅

**问题：无**

---

### Phase 3：高级功能

#### ✅ G9 — 历史管理策略

**状态：已实现**

- `TrimmerConfig`：`prompt.rs:742-762` ✅
- `HistoryTrimmer`：`prompt.rs:765-905` ✅
- `TrimResult`：`prompt.rs:770-779` ✅
- `estimate_tokens()`（chars/3 粗估）：`prompt.rs:790-808` ✅
- `trim()` 方法含保护策略：`prompt.rs:823-904` ✅
- `config.rs` 中 `TrimmerConfigToml`：✅

**🔴 R6：`HistoryTrimmer::trim()` 裁剪逻辑存在状态机缺陷**

`prompt.rs:862-877` 中的裁剪循环：

```rust
let mut keeping = false;
for msg in trimmable.iter().rev() {
    let msg_tokens = Self::estimate_tokens(&[msg.clone()]);
    if !keeping && msg_tokens <= remaining_budget {
        remaining_budget -= msg_tokens;
        kept_trimmable.push(msg.clone());
    } else if keeping {
        kept_trimmable.push(msg.clone()); // ⚠️ 无预算检查
    } else {
        removed_count += 1;
        keeping = false; // ⚠️ 此处 keeping 已经是 false，赋值无意义
    }
}
```

问题分析：
1. `keeping` 变量初始为 `false`，且只在 `else` 分支中被赋值为 `false`，**永远不会变成 `true`**。这意味着 `else if keeping` 分支永远不会执行。
2. 设计意图是"一旦开始保留，后续都保留（避免中间断开）"，但实际实现中 `keeping` 从未被设为 `true`。
3. 结果：算法退化为"从后往前逐条检查是否在预算内，超出则直接移除"，不会出现中间断开的问题（因为是从后往前扫描），但裁剪行为可能与设计预期不一致 —— 它会跳过中间的大消息而保留更早的小消息，产生不连续的历史。

**修复建议**：

```rust
for msg in trimmable.iter().rev() {
    let msg_tokens = Self::estimate_tokens(&[msg.clone()]);
    if msg_tokens <= remaining_budget {
        remaining_budget -= msg_tokens;
        kept_trimmable.push(msg.clone());
    } else {
        removed_count += 1;
        // 一旦有消息被移除，剩余更早的消息也应被移除
        // 避免历史中间出现断裂
        break; // 或者 continue 取决于需要的行为
    }
}
// 在 break 之后，将 trimmable 中剩余更早的消息全部计入 removed_count
```

---

**🔴 R7：`trim_history()` 在 `agent.rs` 中使用硬编码的空 `PromptConfig` 来构建 system prompt**

`agent.rs:703-708`：

```rust
let mut prompt_config = crate::prompt::PromptConfig::new(
    "agent".to_string(),
    String::new(),          // ⚠️ agent_prompt 为空
    std::path::PathBuf::from("."),  // ⚠️ workspace 为当前目录
);
```

这意味着 `trim_history()` 在估算 system prompt token 时使用的是一个**几乎为空的 prompt**，导致历史预算计算不准确（偏大），裁剪阈值过于宽松。

**建议**：`trim_history()` 应该接收实际的 system prompt 字符串或 `PromptConfig`，而非内部构造一个空的。

---

**🟡 R8：`TrimmerConfig` 构造使用 `max_tokens.saturating_mul(16)` 粗估 context_window**

`agent.rs:693`：

```rust
context_window: self.config.max_tokens.saturating_mul(16),
```

`max_tokens` 默认值为 4096（`config.rs` 中的 `default_max_tokens()`），乘以 16 = 65536。但实际模型的 context_window 可能是 128K 或 200K。这个粗估值偏小，可能导致不必要的过早裁剪。

**建议**：从 `config.toml` 的 `[gateway.trimmer].context_window` 读取实际值，而非基于 `max_tokens` 推算。当前 `TrimmerConfigToml` 已定义但未在此处使用。

---

**🟢 R9：`HistoryTrimmer` 缺少测试**

设计文档列出了 6 个测试用例（`trim_no_op_when_under_budget` 等），均未实现。

---

#### ✅ G10 — 侧信道注入

**状态：已实现**

- `SideChannelConfig`：`prompt.rs:912-933` ✅
- `SideChannelInjector`：`prompt.rs:936-1006` ✅
- `bootstrap.rs` 创建并注入：`bootstrap.rs:105-113` ✅
- `agent.rs` 字段定义：`agent.rs:35` ✅
- `config.rs` 中 `SideChannelConfigToml`：✅

**🟡 R10：侧信道注入器未在 `execute_tool_calls()` 中使用**

设计文档 Phase 3 §3.2.3 明确要求在工具结果返回时注入侧信道内容。但 `agent.rs:86-162` 中的 `execute_tool_calls()` 方法未调用 `side_channel_injector.inject_into_tool_result()`。

`SideChannelInjector` 虽已创建并挂载到 `AgentRuntime`，但实际注入点未接入工具执行流程。侧信道功能处于**定义但未生效**的状态。

**修复位置**：`agent.rs:124` 的 tool result 构建处，需要在 `(out.content, out.is_error)` 之后调用注入器。

---

**🟢 R11：`SideChannelInjector` 缺少测试**

设计文档列出了 3 个测试用例（`side_channel_disabled_returns_none` 等），均未实现。

---

#### ✅ G11 — prepare_turn 接入主流程

**状态：已实现**

- `prepare_turn()`：`agent.rs:383-427` ✅
- `run_turn_with_context()` 完整实现（含工具执行、usage 统计、MaxTokens 续写、cancellation）：`agent.rs:435-618` ✅
- `execute_tool_calls()` 共享方法：`agent.rs:86-162` ✅
- `conversation_service.rs` 双路径切换：`conversation_service.rs:92-126` ✅
- `use_turn_context` 配置开关：`config.rs` 中 `GatewayConfig.use_turn_context` ✅
- `AgentConfig.use_turn_context`：`agent.rs:46` ✅

**🟡 R12：`run_turn_with_context()` 参数 `_message` 未使用**

`agent.rs:438`：

```rust
pub async fn run_turn_with_context(
    &self,
    ctx: TurnContext,
    _message: Message,    // ⚠️ 下划线前缀表示未使用
    event_tx: mpsc::Sender<crate::event::AgentEvent>,
    cancellation_token: Option<CancellationToken>,
) -> Result<TurnResult> {
```

`_message` 参数（用户消息）被声明但未添加到 `all_messages` 中。对比 `conversation_service.rs:101-106`，调用方构造了 `user_message` 并传入，但方法内部从未使用它。

然而在 `conversation_service.rs:68-75` 中，用户消息已在调用 `run_turn_with_context` **之前**通过 `append_message` 添加到 session history。而 `prepare_turn()` 获取的 `history_for_turn` 不包含最新用户消息（`history[..history.len() - 1]`）。

这意味着在 `use_turn_context` 路径下，用户消息既没有在 history 中（被裁掉了最后一条），也没有在 `run_turn_with_context` 中被追加，**LLM 看不到当前用户输入**。

**这是一个功能性 bug**。

对比旧路径 `run_turn()`，用户输入通过 `user_input` 参数在 `agent.rs:175-180` 中被显式追加到 `all_messages`。

**修复方案**：在 `run_turn_with_context()` 方法开始处追加 `_message`：

```rust
let mut all_messages = Arc::try_unwrap(ctx.history)
    .unwrap_or_else(|h| (*h).clone());
all_messages.push(_message); // 追加用户消息
```

---

#### 附加 — Workflow 阶段加载

**状态：已实现结构体，但未接入**

- `WorkflowStagePrompts`：`prompt.rs:1012-1068` ✅
- `load_from_file()`, `get()`, `render()` 方法：✅

**🟡 R13：`WorkflowStagePrompts` 在 `from_config()` 中未被调用**

设计文档 Phase 3 §5.2.2 要求在 `from_config()` 中当 `workflow_stage != "idle"` 时加载 workflow prompt 并注入到 `Workflow` section。但当前 `from_config()` 中没有此逻辑。

`WorkflowStagePrompts` 处于"定义但未使用"状态，属于预留实现。

---

**🟢 R14：`WorkflowStagePrompts::load_from_file()` 只提取代码块内容**

`prompt.rs:1038-1045` 中的解析逻辑只收集 ` ``` ` 围栏内的内容，忽略围栏外的普通文本。这意味着 workflow-stages.md 中如果 prompt 内容不在代码块内，会被丢弃。这是设计意图但需要在文档中明确说明。

---

## 三、问题汇总与优先级

| 编号 | 等级 | 位置 | 问题摘要 | 影响 |
|------|------|------|----------|------|
| R1 | 🟡 | prompt.rs | EnvironmentSnapshot 缺少测试 | 质量 |
| R2 | 🟡 | prompt.rs:269 | load_project_context 使用同步 I/O | 规范违反 |
| R3 | 🟡 | config.rs | 缺少 project_context_file 自定义路径 | 设计偏差（可选功能） |
| R4 | 🟢 | prompt.rs | load_project_context 缺少测试 | 质量 |
| R5 | 🔴 | — | 需确认无残留 generate_system_prompt() 调用 | 编译警告 |
| R6 | 🔴 | prompt.rs:862 | HistoryTrimmer 裁剪循环 keeping 状态机无效 | 逻辑缺陷 |
| R7 | 🔴 | agent.rs:703 | trim_history 使用空 PromptConfig 估算 | 裁剪不准 |
| R8 | 🟡 | agent.rs:693 | context_window 粗估偏小 | 过早裁剪 |
| R9 | 🟢 | prompt.rs | HistoryTrimmer 缺少测试 | 质量 |
| R10 | 🟡 | agent.rs | 侧信道注入器未接入工具执行流 | 功能未生效 |
| R11 | 🟢 | prompt.rs | SideChannelInjector 缺少测试 | 质量 |
| R12 | 🟡 | agent.rs:438 | run_turn_with_context _message 未使用，用户消息丢失 | 功能 bug（新路径） |
| R13 | 🟡 | prompt.rs | WorkflowStagePrompts 未接入 from_config | 功能未生效 |
| R14 | 🟢 | prompt.rs:1038 | WorkflowStagePrompts 只提取代码块内容 | 需文档说明 |

---

## 四、建议修复顺序

### 第一优先级（功能缺陷）

1. **R12** — `run_turn_with_context` 用户消息丢失。这是 `use_turn_context` 路径下的阻塞性 bug，必须在启用新路径前修复。
2. **R6** — `HistoryTrimmer` keeping 状态机无效。裁剪行为虽然不会崩溃，但可能产生不连续的历史。
3. **R7** + **R8** — `trim_history` 使用空 PromptConfig 和粗估 context_window。应从配置中读取 `TrimmerConfigToml` 的实际值。

### 第二优先级（功能完整性）

4. **R10** — 侧信道注入器接入 `execute_tool_calls()`。
5. **R5** — 确认无残留 `generate_system_prompt()` 调用。
6. **R13** — WorkflowStagePrompts 接入 from_config（可延后到实际使用 workflow 时）。

### 第三优先级（质量改进）

7. **R1, R4, R9, R11** — 补充测试覆盖。
8. **R2** — 同步 I/O 问题（影响有限，可延后）。
9. **R3** — project_context_file 自定义路径（可选功能）。
10. **R14** — 补充 WorkflowStagePrompts 解析行为文档说明。

---

## 五、设计文档完成度总结

| Phase | 设计目标 | 代码实现 | 测试覆盖 | 遗留问题 |
|-------|----------|----------|----------|----------|
| Phase 1 (G1-G4) | ✅ 全部实现 | ✅ 全部接入 | ⚠️ 基本覆盖 | 无 |
| Phase 2 (G5-G8) | ✅ 全部实现 | ✅ 全部接入 | ⚠️ 部分缺失 | R1-R4 |
| Phase 3 (G9-G11+附加) | ✅ 全部实现 | ⚠️ 部分未接入 | ❌ 几乎无测试 | R6-R13 |

Phase 1 和 Phase 2 的实现质量较高，代码与设计文档基本一致。Phase 3 的代码结构完整，但存在多个功能性 bug（R6、R7、R12）和未接入的功能点（R10、R13），建议在启用 `use_turn_context` 之前完成修复。
