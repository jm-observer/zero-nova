# 工具元信息提示词优化设计总览

## 时间
- 创建时间：2026-05-13
- 最后更新：2026-05-13

## 项目现状
- `nova-agent` 当前存在两类工具信息暴露渠道：
  - provider tool 参数始终承载结构化工具定义，见 `D:\git\zero-nova\crates\nova-agent\src\provider\anthropic.rs:115`、`D:\git\zero-nova\crates\nova-agent\src\provider\openai_compat\conv.rs:156`
  - system prompt 在部分构建路径下会附带工具说明文本，构建位置见 `D:\git\zero-nova\crates\nova-agent\src\prompt\mod.rs:972`
- `SystemPromptBuilder::with_tool_definitions_internal(...)` 在 `ToolGuidanceMode::Full` 下会把每个工具的完整 `input_schema` 直接内联到 prompt 中，说明代码路径具备 “prompt 携带完整 schema” 的能力；但最近一次实际请求样本中，system prompt 仅包含通用工具能力摘要，逐工具 schema 仍主要由 provider tools 承载。
- `ToolRegistry` 已经具备统一的工具元数据来源：
  - loaded 工具定义：`D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:127`
  - deferred 工具定义与类别：`D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:136`
  - 当前轮工具视图：`D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:461`
- 当前已有 `ToolSearch` 作为 deferred 工具的发现 / 加载入口，见 `D:\git\zero-nova\crates\nova-agent\src\tool\builtin\tool_search.rs:7`，说明系统已经具备“按需暴露工具能力”的基础机制。

## 整体目标
- 将 prompt 中的工具内容收敛为“名称 + 描述 + 使用引导”，不再内联完整 schema。
- 新增一个只读的 `ToolInfo` 工具，供模型按需检索具体工具的完整元信息（如参数、required 字段、schema）。
- 保持 provider 层的 tool schema 不变，避免破坏现有 tool-calling 协议和 schema 校验链路。
- 让工具发现、工具详情查询和实际工具执行形成清晰的分层：
  - 工具选择看 prompt 描述
  - 参数细节查 `ToolInfo`
  - deferred 工具发现 / 加载仍走 `ToolSearch`

## 非目标
- 不移除 provider request 中的 `tools[].input_schema / parameters`。
- 不改变 `ToolDefinition` 到 Anthropic / OpenAI-compatible request 的映射方式。
- 不重构现有 `ToolSearch` 为统一大而全入口；本次只补 `ToolInfo` 能力。
- 不修改 `schema_validation` 的执行逻辑，不改变真实工具执行时的入参校验语义。
- 不新增第三方依赖。

## 关键约束
- `main.rs` 之外保持 `anyhow::Result` + `?` 的错误传播风格。
- 新增工具若涉及运行时上下文过滤，必须复用现有 `ToolContext` / runtime 注入链路，而不是绕开 capability policy 直接访问全局注册表。
- 对模型可见的 prompt 内容必须与当前轮可见工具集合一致，避免描述、查询和执行三者可见性不一致。
- 所有代码改动完成后，必须执行完整检查链路：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all`
  - `cargo test --workspace`

## Plan 拆分
| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | ToolRegistry 元信息视图与 ToolInfo 工具设计 | 无 | 1 | 已完成 |
| Plan 2 | Prompt 工具展示降维与 ToolInfo 使用引导 | Plan 1 | 2 | 已完成 |
| Plan 3 | 测试补齐、兼容性回归与完整校验 | Plan 1, Plan 2 | 3 | 已完成 |

执行顺序说明：
- Plan 1 先建立统一元数据视图和查询入口，否则 Plan 2 只能继续依赖 prompt 内嵌 schema 或额外拼接逻辑。
- Plan 2 在 Plan 1 之后进行，以便 prompt 中的使用引导能直接指向稳定存在的 `ToolInfo` 工具。
- Plan 3 最后执行，统一验证 prompt、provider、registry、runtime 之间的契约没有回归。

## 推荐方案摘要
- 保留 provider `tools` 参数中的完整 schema，仅移除 prompt 中的 schema 内联。
- 在 `ToolRegistry` 中新增统一的 `ToolMetadataView` 查询能力，供 `ToolInfo` 和未来诊断逻辑复用。
- 新增内置 `ToolInfo` 工具：
  - 精确按工具名查询
  - 返回完整 schema 与 required 字段摘要
  - 首版不自动 load deferred tool
  - 只返回当前轮可见工具的元信息
- 改造 `SystemPromptBuilder` 的工具 section：
  - 保留工具描述
  - 增加固定提示：不清楚参数时先调 `ToolInfo`
  - 不再把 `input_schema` 直接放入 prompt

## 验收标准
- prompt 中不再出现完整工具 schema JSON。
- provider request 仍保留完整 `tools[].input_schema / parameters`。
- `ToolInfo` 能返回 loaded / deferred 工具的完整元信息。
- `ToolInfo` 不会暴露当前轮不可见工具。
- `ToolSearch` 的现有 search / select / load 行为不回归。
- 完整检查链路全部通过。

## 风险与待定项
- 风险 1：若 prompt 去 schema 后缺少足够引导，模型可能在第一次尝试时仍猜测参数。
- 风险 2：若 `ToolInfo` 直接读取全局 registry 而不经过当前轮可见性过滤，可能泄漏受限工具。
- 风险 3：prompt 体积诊断与调试快照当前仍按 schema 统计，可能与“用户实际看到的 prompt 变短”产生认知偏差。
- 待定项 1：`ToolGuidanceMode` 是直接复用现有 `Full` 语义，还是新增更明确的 `DescriptionOnly` / `DescriptionWithLookupHint` 模式。
- 待定项 2：`ToolInfo` 输出是以 JSON 为主，还是“文本摘要 + JSON 块”双层结构；首版建议后者，兼顾模型可读性与结构化细节。
