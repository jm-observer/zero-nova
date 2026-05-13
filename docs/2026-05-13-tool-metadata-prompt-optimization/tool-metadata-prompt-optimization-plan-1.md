# Plan 1: ToolRegistry 元信息视图与 ToolInfo 工具设计

> **状态：已完成** (2026-05-13)

## Plan 编号与标题
- Plan 1: ToolRegistry 元信息视图与 ToolInfo 工具设计

## 前置依赖
- 无

## 本次目标
- 在 `ToolRegistry` 中建立统一的工具元信息查询视图，避免 prompt、工具查询工具和调试逻辑各自拼接 schema 数据。
- 新增一个只读的 `ToolInfo` 工具，作为模型按需查询工具参数与 schema 的标准入口。
- 确保 `ToolInfo` 只返回当前轮可见工具的元信息，不绕过 capability policy。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/builtin/mod.rs`
- `crates/nova-agent/src/tool/builtin/tool_info.rs`（新增）
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/agent/runtime/tool_exec.rs`

## 详细设计
### 1. 为 ToolRegistry 增加统一的元信息视图
- 当前 `ToolRegistry` 已维护：
  - loaded definitions：`D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:127`
  - deferred representations：`D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:146`
- 在此基础上新增统一结构，例如：
  - `ToolMetadataView`
    - `name: String`
    - `description: String`
    - `input_schema: Value`
    - `loaded: bool`
    - `deferred: bool`
    - `category: Option<DeferredToolCategory>`
- 推荐新增接口：
  - `pub fn tool_metadata(&self, name: &str) -> Option<ToolMetadataView>`
  - `pub fn all_tool_metadata(&self) -> Vec<ToolMetadataView>`
- 实现规则：
  - loaded 工具从 `snapshot.loaded_definitions` 生成。
  - deferred 工具从 `snapshot.deferred_representations` 生成。
  - 若同名工具同时出现在 loaded / deferred 集合中，loaded 优先。
- 不修改现有 `tool_definitions()` 行为，避免影响 provider 调用链。

### 2. 新增 ToolInfo 工具
- 新建 `tool/builtin/tool_info.rs`，实现 `Tool` trait。
- 工具定义建议：
  - 名称：`ToolInfo`
  - 描述：`Retrieve complete metadata for one or more tools, including full input schema and required parameters.`
- 输入 schema 建议：
  - `tool_names: string[]`（必填）
  - `include_schema: boolean`（可选，默认 true）
- 输出内容建议为“摘要文本 + JSON 块”：
  - tool name
  - description
  - loaded / deferred
  - category
  - required_fields
  - field_summaries
  - input_schema（可选）
- 首版不自动 load deferred tool，因为 `DeferredToolEntry` 已保留 `input_schema`，查询工具详情并不依赖实例化。

### 3. 当前轮可见性约束
- `AgentRuntime::prepare_turn(...)` 在 `D:\git\zero-nova\crates\nova-agent\src\agent\runtime.rs:216` 已经生成当前轮工具集合。
- 为避免 `ToolInfo` 泄漏当前轮不可见工具，推荐在 `ToolContext` 中增加：
  - `visible_tool_names: Arc<HashSet<String>>`
- 在工具执行上下文构造时注入当前轮可见工具名集合。
- `ToolInfo` 执行时先检查查询目标是否在 `visible_tool_names` 中，仅返回允许查询的工具。

### 4. 内置工具注册
- 在 `tool/builtin/mod.rs` 中：
  - 增加 `pub mod tool_info;`
  - 在 `register_builtin_tools(...)` 中注册 `ToolInfo`
- 推荐作为 loaded 工具直接注册，而不是 deferred：
  - 这是 schema 查询基础设施，应默认可见。
  - 若它本身也是 deferred，会让“先查参数”这条路径变得不稳定。

## 测试案例
- 正常路径：
  - 查询单个 loaded 工具，返回完整元信息与 schema。
  - 查询单个 deferred 工具，在不 load 的情况下也能返回 schema。
  - 查询多个工具，返回顺序稳定。
- 边界条件：
  - 查询工具名为空数组时返回明确错误。
  - 查询已知工具但 `include_schema=false` 时，仅返回摘要字段。
  - loaded / deferred 同名时返回 loaded 版本。
- 异常场景：
  - 查询当前轮不可见工具，返回拒绝或 not visible 提示。
  - 查询不存在工具，返回明确 not found 信息。
