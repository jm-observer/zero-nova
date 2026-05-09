# Prompt Compaction 设计

- **创建日期**: 2026-05-09
- **最后更新**: 2026-05-09
- **状态**: 待评审

---

## 项目现状

当前 `nova-agent` 的请求上下文由多类信息拼接形成：

1. `.nova/prompts/agent-developer.md` 作为子代理基础身份与行为提示词。
2. `AGENTS.md` 通过 `Developer Project Instructions` section 注入项目级开发规则。
3. `SkillRegistry` 生成可用 skill 列表，system prompt 中会包含 skill 触发说明。
4. 请求顶层 `tools` 字段携带完整工具 schema。
5. 对话历史会保留 assistant tool call 与 tool result，后续轮次继续回灌。

从 `tmp/response` 的实际请求看，当前体量主要分布为：

| 来源 | 现状 | 风险 |
|---|---|---|
| system prompt | 约 14.9K 字符 | 规则重复、任务焦点被稀释 |
| tools schema | 约 5.1K 字符 | 必要但不应在 prompt 文本重复描述 |
| 历史 tool 输出 | 单条约 33.5K 字符 | 最大 token 消耗来源，会吞掉后续上下文预算 |
| 请求参数 | 同时发送 `max_completion_tokens` 和 `max_tokens` | OpenAI-compatible 服务行为不一致 |

关键代码位置：

1. `crates/nova-agent/src/prompt.rs`：`SystemPromptBuilder::from_config` 负责拼接 Base、BehaviorGuards、Skill、DeveloperProjectPrompt、ProjectContext、Environment、AgentCatalog、Workflow 等 section。
2. `crates/nova-agent/src/agent.rs`：负责准备 turn、裁剪 history、执行工具并将 tool result 写回历史。
3. `crates/nova-agent/src/provider/openai_compat/conv.rs`：负责将内部消息与模型配置转换为 OpenAI-compatible 请求。
4. `.nova/prompts/agent-developer.md`：当前包含身份、工作方式、搜索命令、工程约束、交付要求等内容。
5. `AGENTS.md`：当前包含完整项目工程规范、设计文档规则、修复流程和执行约束。

---

## 整体目标

本设计目标是在不改变 agent 基本行为的前提下，降低单轮请求上下文体量，并提升提示词优先级与来源边界清晰度。

具体目标：

1. 建立可观测的 prompt/token 体量统计，先量化再优化。
2. 将规则块按职责分层，减少 `agent-developer.md` 与 `AGENTS.md` 的重复注入。
3. 对历史 tool result 做结构化压缩，优先解决最大上下文来源。
4. 对 skills 与 tools 采用“目录常驻、详情按需”的注入策略。
5. 对 OpenAI-compatible 请求参数做 provider-aware 治理，避免无条件双写。
6. 保持 prompt preview、调试命令和实际请求一致，避免只优化某一条链路。

---

## 设计原则

1. **先压历史，再压规则**：实际请求中历史工具输出最大，优先裁剪 tool result，比手工删规则收益更稳定。
2. **职责单一**：`agent-developer.md` 描述子代理角色，`AGENTS.md` 描述项目规则，skill 文件描述专项能力，tool schema 只通过请求工具定义传递。
3. **按需注入**：不把所有规则永久放进 system prompt，仅在任务类型或路由结果需要时注入。
4. **可回溯**：任何压缩都要保留来源、原始长度、截断说明，便于用户判断是否需要重新读取完整内容。
5. **保守兼容**：默认不破坏现有 agent 行为，新增配置先采用默认开启安全压缩、可配置阈值。
6. **测试覆盖**：每个压缩规则都要覆盖正常路径、边界长度、超长输出和多字节字符场景。

---

## Plan 拆分

| Plan | 标题 | 简要描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|---|
| Plan 1 | Prompt 体量诊断 | 增加 section、tools、history 的字符/token 估算与调试输出 | 无 | 1 | 待开始 |
| Plan 2 | 规则块分层精简 | 重划 `agent-developer.md`、`AGENTS.md`、skills、tools 的注入边界 | Plan 1 | 2 | 待开始 |
| Plan 3 | 历史 Tool 输出压缩 | 对超长 tool result 进行结构化压缩，并接入 history 回灌链路 | Plan 1 | 3 | 待开始 |
| Plan 4 | 请求参数与回归验证 | 治理 `max_tokens` 双写策略，补齐端到端请求快照测试 | Plan 2, Plan 3 | 4 | 已完成 |

执行顺序说明：

1. Plan 1 先提供度量基线，避免盲目精简。
2. Plan 2 处理 system prompt 内部重复和职责边界。
3. Plan 3 处理实际最大体量来源，即历史 tool 输出。
4. Plan 4 收口 provider 请求兼容性与回归测试。

---

## 风险与待定项

| 风险 | 描述 | 缓解策略 |
|---|---|---|
| 规则缺失 | 过度精简 `AGENTS.md` 可能导致 agent 漏遵守项目约束 | 按任务类型保留必要规则，并在 prompt preview 中可见 |
| 工具输出信息损失 | 压缩 tool result 后模型可能缺少完整文件上下文 | 保留摘要、头尾片段和重新读取提示，必要时允许特定工具结果不压缩 |
| provider 兼容差异 | 有些 OpenAI-compatible 服务需要 `max_tokens`，有些不接受双写 | 增加 provider 配置项或兼容模式，默认保持当前兼容行为再逐步切换 |
| token 估算偏差 | 字符数和真实 token 不完全一致 | 使用字符数作为轻量运行时指标，测试中只验证相对下降 |
| UI preview 不一致 | 只改请求链路可能导致 preview 仍显示旧内容 | 统一通过 `PromptConfig` 和 `SystemPromptBuilder` 生成 |

---

## 非目标

本次设计不处理以下内容：

1. 不重构 agent loop 的整体执行模型。
2. 不改变工具调用协议和 tool schema 格式。
3. 不引入新的 tokenizer 依赖，除非后续确认字符估算不足。
4. 不把 `AGENTS.md` 改造成复杂 DSL。
5. 不改变用户显式要求完整读取文件时的工具执行行为，只改变回灌给模型的历史表达。
