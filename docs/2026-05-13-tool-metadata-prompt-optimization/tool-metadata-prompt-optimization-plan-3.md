# Plan 3: 测试补齐、兼容性回归与完整校验

> **状态：已完成** (2026-05-13)

## Plan 编号与标题
- Plan 3: 测试补齐、兼容性回归与完整校验

## 前置依赖
- Plan 1: ToolRegistry 元信息视图与 ToolInfo 工具设计
- Plan 2: Prompt 工具展示降维与 ToolInfo 使用引导

## 本次目标
- 验证 registry、prompt、runtime、provider 四条链路在变更后保持契约一致。
- 确保 prompt 去 schema 不会影响 provider tool schema、真实工具执行和参数校验。
- 补齐 `ToolInfo`、prompt 文本变化和可见性边界的回归测试。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/builtin/tool_info.rs`
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/provider/openai_compat/conv.rs`
- `crates/nova-agent/tests/integration/*`（按实际需要补充）

## 详细设计
### 1. ToolRegistry / ToolInfo 单元测试
- 为 `ToolRegistry` 新增元信息查询测试：
  - loaded 工具可查询
  - deferred 工具可查询
  - loaded / deferred 同名时 loaded 优先
- 为 `ToolInfo` 新增测试：
  - 单工具查询
  - 多工具查询
  - not found
  - 当前轮可见性限制

### 2. Prompt 单元测试
- 调整 `prompt/mod.rs` 中现有测试：
  - 新模式下只输出描述，不内联 schema JSON。
  - ToolGuidance section 包含 `ToolInfo` 使用引导。
- 保持 `Compact` 模式现有输出语义不变。

### 3. Provider / runtime 集成回归
- 验证 prompt 中不再包含 schema 文本。
- 验证 provider request body 仍保留完整 `tools[].input_schema / parameters`。
- 验证真实工具执行仍走 `schema_validation`，见 `D:\git\zero-nova\crates\nova-agent\src\tool\registry.rs:623`。
- 若具备合适测试替身，补一个“先 `ToolInfo` 后真实工具调用”的回归路径。

### 4. 完整检查链路
- 完成代码改动后，必须完整执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all`
  - `cargo test --workspace`
- 按仓库要求，三步必须全部完成，不能在中途停止。

## 测试案例
- 正常路径：
  - 模型可见 prompt 中只有描述，provider 仍带完整 schema。
  - `ToolInfo` 返回 schema 后，真实工具执行仍能通过 schema 校验。
- 边界条件：
  - deferred 工具未 load 时也可查询 schema。
  - provider tools 为空时，prompt 与 request 都保持稳定。
- 异常场景：
  - 不可见工具查询被拒绝。
  - 旧测试若仍依赖 schema 出现在 prompt 中，需同步更新并证明行为调整是预期的。
