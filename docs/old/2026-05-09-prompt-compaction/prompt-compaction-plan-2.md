# Plan 2: 规则块分层精简

## 前置依赖

Plan 1: Prompt 体量诊断。

---

## 本次目标

减少 system prompt 中规则重复和职责混杂，让不同来源的规则各司其职，并支持按任务类型注入必要子集。

可验证目标：

1. `agent-developer.md` 只保留 agent 身份、通用工作方式、交付要求。
2. `AGENTS.md` 不再整篇无差别注入，可按任务类型选择规则片段。
3. skills 默认只展示目录摘要，命中后再注入完整 skill 指令。
4. tools 不再在 system prompt 中重复完整 schema。
5. prompt preview 与实际请求使用相同分层结果。

---

## 涉及文件

| 文件 | 变更类型 | 说明 |
|---|---|---|
| `.nova/prompts/agent-developer.md` | 修改 | 精简 agent 基础提示词 |
| `AGENTS.md` | 可选修改 | 如需支持片段标记，可增加轻量 section 约定 |
| `crates/nova-agent/src/prompt.rs` | 修改 | 支持 project instructions profile 和 section 过滤 |
| `crates/nova-agent/src/skill.rs` | 修改 | 支持 skill catalog 与 full skill 分离 |
| `crates/nova-agent/src/config.rs` | 修改 | 增加 prompt compaction/profile 配置 |
| `crates/nova-agent/tests/integration/*` | 新增/修改 | 覆盖不同任务类型的注入结果 |

---

## 详细设计

### 1. 规则来源边界

建议明确四层来源：

| 层级 | 来源 | 应包含 | 不应包含 |
|---|---|---|---|
| L0 Agent Base | `.nova/prompts/agent-developer.md` | 身份、职责、工作方式、交付摘要 | 项目 Rust 细则、完整修复流程、设计文档模板 |
| L1 Behavior Guards | 内置常量 | 工具行动一致性等硬约束 | 项目特定规范 |
| L2 Project Instructions | `AGENTS.md` | 与当前任务相关的项目规则 | 与任务无关的完整长文档 |
| L3 Skills | skill registry | skill 名称和触发描述 | 未命中 skill 的完整正文 |
| L4 Tools | request `tools` | schema 定义 | system prompt 中重复 schema |

### 2. 精简 `agent-developer.md`

建议保留：

1. Nova Developer 身份。
2. 中文交流。
3. 修改前先读文件。
4. 小步聚焦，不混入重构。
5. 信息不足时停止并说明阻塞。
6. 输出修改摘要和验证结果。
7. `rg` 基本使用约束。
8. Windows UTF-8 输出约束可保留精简版，因为这是工具执行稳定性问题。

建议移出或删除：

1. Rust 日志、错误处理、tokio、unwrap 等细则：由 `AGENTS.md` 按代码修改任务注入。
2. 详细 ripgrep 反例说明：保留一条“用 `-g`，不用 `--include`”即可。
3. 完整交付格式模板：保留摘要要求即可。

### 3. `AGENTS.md` profile 化

首版不建议改造为复杂 DSL。可以在加载后通过标题匹配做 profile 抽取：

| Profile | 触发场景 | 注入章节 |
|---|---|---|
| `analysis` | 用户只要求分析、解释、定位 | 基本行为、代码结构、必要搜索约束 |
| `code` | 用户要求修改/新增/修复代码 | 基本行为、技术栈、代码质量、修复流程 |
| `design` | 用户要求计划/设计文档 | 基本行为、计划与设计文档 |
| `review` | 用户要求 review | 基本行为、代码质量、测试要求 |
| `full` | 用户显式要求完整规则 | 全文 |

任务类型可以先由调用方显式传入，后续再做自动识别。

### 4. Skill 注入分层

当前 system prompt 中 skill 描述可能随着 skill 数量增长而膨胀。建议拆为两类：

1. `SkillCatalog`：默认注入，只包含 skill id、display name、一句话 description、触发方式。
2. `ActiveSkillInstructions`：仅 active skill 或用户显式 `/skill-name` 时注入完整 `SKILL.md`。

如果没有 active skill，则 system prompt 不包含完整 skill 正文。

### 5. Tool 指导与 schema 去重

顶层 `tools` 已经包含完整 schema，因此 system prompt 中不应重复 schema。

保留的 system tool guidance 只描述策略：

1. 修改文件前先读文件。
2. 搜索优先 `rg`。
3. 不要声称执行了未调用的工具。
4. 大输出应先缩小范围。

工具参数、字段、schema 全部交给 `tools` 字段。

### 6. 配置建议

```toml
[prompt_compaction]
enabled = true
project_instruction_profile = "auto"
skill_injection = "catalog"
tool_guidance = "compact"
```

可选值：

1. `project_instruction_profile`: `auto | analysis | code | design | review | full`
2. `skill_injection`: `catalog | active_full | full`
3. `tool_guidance`: `compact | full`

---

## 测试案例

| 类型 | 用例 | 期望 |
|---|---|---|
| 正常路径 | code profile | 包含代码质量和修复流程，不包含完整设计文档模板 |
| 正常路径 | design profile | 包含设计文档规则，不包含完整 Rust 技术栈细则 |
| 边界条件 | AGENTS.md 缺少预期标题 | 回退到 compact 摘要或全文，且记录 debug 信息 |
| 异常场景 | skill 很多但未激活 | 只注入 catalog，不注入完整 skill 正文 |
| 显式触发 | 用户使用 `/skill-x` | 注入该 skill 完整说明 |
| 兼容性 | compaction disabled | 保持旧的全文注入行为 |

---

## 验收标准

1. 常规 developer 任务 system prompt 字符数显著下降。
2. 关键项目规则仍能按任务类型注入。
3. 未激活 skill 不再注入完整正文。
4. prompt preview 与实际请求一致。
5. 修复流程全部通过。
