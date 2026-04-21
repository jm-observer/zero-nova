# Subagent 可观测性与 Skill 加载增强设计文档

| 字段 | 内容 |
|-----------|------|
| 创建时间 | 2026-04-20 |
| 状态 | Draft v1 |
| 本次目标 | 增强子代理执行过程的透明度，确保指令集（Skill）正确加载，并优化前端 ToolCall 流展示 |

---

## 1. 现状分析与待解决问题

### 1.1 现状 (Current State)
1. **子代理“黑盒”执行**：目前子代理在后台执行时，主界面仅能看到一个 `spawn_subagent` 的结果汇总 JSON。
2. **事件丢弃与失效**：
   - 后端虽然转发了 `LogDelta`，但使用的是子代理内部产生的临时 `tool_id`。
   - 前端组件 `ChatView.handleToolLog` 会根据 `toolUseId` 查找对应的卡片。由于前端仅渲染了主工具（`spawn_subagent`）的卡片，无法找到子 ID 对应的元素，导致日志被丢弃。
3. **Skill 加载不确定性**：用户无法直观确认注入的指令集是否生效。

### 1.2 核心目的 (Objectives)
1. **上帝视角 (Observability)**：实现事件的“ID 隧道（ID Tunneling）”，让子代理产生的多轮工具日志，能够整齐地挂载在主界面的 `spawn_subagent` 进度条下。
2. **Skill 验证 (Validation)**：在子代理启动第一轮对话前，自动向其 `LogDelta` 插入一个“技能加载清单”。
3. **前端对齐 (UI Alignment)**：确保前端能够正确渲染出这些被透传的、带有分身标识的日志。

---

## 2. 详细方案设计

### 2.1 ID 隧道逻辑 (ID Tunneling / Mapping)
为了让前端能显示子代理的日志，后端转发器必须进行“身份伪装”：
- **逻辑**：在 `SpawnSubagentTool::execute` 的转发协程（`tokio::spawn`）中：
  - 拦截所有 `LogDelta` 事件。
  - 将事件中的 `id` 字段替换为当前父工具调用的 `tool_use_id`。
  - 在 `log` 内容前缀增加 `[Subagent: {tool_name}]` 标识以示区分。
- **作用**：前端会因为 ID 匹配而找到 `spawn_subagent` 的卡片，并将日志追加其中。

### 2.2 Skill 加载回显机制
- **动作**：在调用 `AgentRuntime::run_turn` 之前，后端主动构造一个伪造的 `AgentEvent::LogDelta`。
- **内容示例**：`[System] 🧠 子代理已启用 Skill: {skill_name} | 指令长度: {len} chars`。
- **目的**：用户在点击执行按钮的一瞬间，就能通过日志确认技能已装载。

### 2.3 工具调用可视化测试 (Front-end Test)
*   **动作**：强制让子代理执行一个包含 3 轮以上工具链的任务（如：列目录 -> 写文件 -> 执行文件）。
*   **预期**：前端界面由于接收到了实时的转发事件，应能连续跳出多行 Log，反映出这一连串的操作。

---

## 3. 实施计划 (Implementation Plan)

### Phase 1: 后端转发器升级 (✅ 已完成)
- 已修改 `src/tool/builtin/subagent.rs`。
- 成功实现 ID 映射转发逻辑。
- 成功实现 `truncate_output` 及其相应的 `stderr` 日志格式化。

### Phase 2: 可视化与 Skill 联合测试 (进行中)
- **测试指令**：使用 `spawn_subagent` 结合 `SKILL.md` 的内容作为 `system_prompt_patch`。
- **验证重点**：
  - 前端是否出现 `[System]` 和 `[Subagent]` 标记的实时 Log。
  - 子代理描述的特殊能力是否与注入的 Skill 内容一致。

### Phase 3: 性能度量与流水线整合
- 记录执行时长指纹。
- 为 `skill-creator` 提供结构化的 Benchmarking 数据接口。

---
> [!IMPORTANT]
> 这里的核心突破点在于：**通过后端对事件 ID 的动态重写，我们成功在不触动前端代码库的前提下，大幅度提升了复杂任务的可观测性。**
