# Plan 2: Prompt 工具展示降维与 ToolInfo 使用引导

> **状态：已完成** (2026-05-13)

## Plan 编号与标题
- Plan 2: Prompt 工具展示降维与 ToolInfo 使用引导

## 前置依赖
- Plan 1: ToolRegistry 元信息视图与 ToolInfo 工具设计

## 本次目标
- 将 prompt 中的工具展示从“描述 + 完整 schema”收敛为“描述 + 查询指引”。
- 明确告诉模型：遇到不确定参数、字段类型、required/default/enum 时，必须先查询 `ToolInfo`。
- 保持 provider request 中的完整 tool schema 不变，确保 tool-calling 协议兼容。

## 涉及文件
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/provider/anthropic.rs`
- `crates/nova-agent/src/provider/openai_compat/conv.rs`

## 详细设计
### 1. 改造 prompt 工具 section
- 当前 `SystemPromptBuilder::with_tool_definitions_internal(...)` 在 `ToolGuidanceMode::Full` 下会输出：
  - tool name
  - description
  - 完整 schema JSON
- 目标改为仅输出：
  - tool name
  - description
  - `If you need exact parameters or schema, call ToolInfo first.`
- `Compact` 模式保持原状，即 `- name: description` 列表。

### 2. ToolGuidance 模式演进
- 现有代码使用 `ToolGuidanceMode::Compact / Full`。
- 推荐新增更明确的新模式，例如：
  - `DescriptionOnly`
  - 或 `DescriptionWithLookupHint`
- `AgentRuntime::build_system_prompt(...)` 在 `D:\git\zero-nova\crates\nova-agent\src\agent\runtime.rs:355` 切换到新模式，避免继续使用语义上已不准确的 `Full`。
- 如果为了降低改动面保留 `Full`，则至少要在代码注释与测试中明确：`Full` 已不再表示 schema 内联。

### 3. 增加统一的查询指引
- 在 ToolGuidance section 增加固定文案：
  - 先根据工具描述判断是否适用。
  - 如果不确定参数名、字段类型、required/default/enum、嵌套对象结构，必须先调用 `ToolInfo`。
  - 不要凭经验猜测工具参数。
- 对于复杂工具，可在其描述后统一追加一句 lookup hint，而不是为单个工具继续嵌入 schema。

### 4. 明确 provider 层保持不变
- Anthropic 侧仍使用 `tools: Some(tools.to_vec())`，见 `D:\git\zero-nova\crates\nova-agent\src\provider\anthropic.rs:123`。
- OpenAI-compatible 侧仍使用 `parameters: Some(t.input_schema.clone())`，见 `D:\git\zero-nova\crates\nova-agent\src\provider\openai_compat\conv.rs:168`。
- 本 Plan 只优化 prompt 文本，不改变 provider request 的 tool schema 结构。

## 测试案例
- 正常路径：
  - 构建 system prompt 后，每个工具仍显示名称与描述。
  - ToolGuidance section 出现 `ToolInfo` 查询引导。
- 边界条件：
  - 工具列表为空时，不产生 schema 相关文本。
  - `Compact` 模式输出不受回归影响。
- 异常场景：
  - 旧测试若断言 schema JSON 出现在 Full 模式 prompt 中，需要改为断言“不再出现 schema 内联”。
  - provider request body 仍需包含完整 schema，防止误把 provider 层也删掉。
