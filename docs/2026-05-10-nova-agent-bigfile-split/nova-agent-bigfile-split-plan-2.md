# Plan 2: prompt.rs 与 config.rs 拆分设计

## 前置依赖
- Plan 1

## 本次目标
- 将 `prompt.rs` 与 `config.rs` 从“单文件大而全”拆为“按职责分层”的子模块结构。
- 保持已有公共类型与函数签名稳定，优先使用模块重组而非行为重写。
- 为后续 `agent` 与 `skill` 层提供更清晰的配置/提示词边界。

## 涉及文件
- `crates/nova-agent/src/prompt.rs`
- `crates/nova-agent/src/prompt/mod.rs`（新增）
- `crates/nova-agent/src/prompt/types.rs`（新增）
- `crates/nova-agent/src/prompt/builder.rs`（新增）
- `crates/nova-agent/src/prompt/templates.rs`（新增）
- `crates/nova-agent/src/prompt/routing.rs`（新增）
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/config/mod.rs`（新增）
- `crates/nova-agent/src/config/models.rs`（新增）
- `crates/nova-agent/src/config/loaders.rs`（新增）
- `crates/nova-agent/src/config/validation.rs`（新增）

## 详细设计
1. `prompt` 模块拆分
- `types.rs`：承载 `TurnContext`、`SkillRouteDecision`、`ActiveSkillState` 等纯数据结构。
- `templates.rs`：承载静态模板文本、模板拼装片段、常量定义。
- `routing.rs`：承载 skill 路由决策、上下文裁剪、能力开关判断。
- `builder.rs`：仅负责 `SystemPromptBuilder` 的编排流程，调用 `types/templates/routing`。
- `mod.rs`：对外 re-export 现有公共类型，确保外部 `crate::prompt::Xxx` 不变。

2. `config` 模块拆分
- `models.rs`：配置结构体、枚举、默认值构造。
- `loaders.rs`：文件读取、环境变量覆盖、反序列化。
- `validation.rs`：配置合法性校验、错误上下文。
- `mod.rs`：保留原导出面并协调加载流程。

3. 边界约束
- IO 与校验解耦：loaders 不做业务校验，validation 不做 IO。
- 避免跨层反向依赖：`builder` 不直接依赖 `agent`，只消费 `types`。
- 控制可见性：内部 helper 默认私有，仅在 `mod.rs` 做最小公开。

## 测试案例
- 正常路径：配置加载 + prompt 构建主流程回归测试通过。
- 边界条件：空配置、缺省配置、最小上下文构建 prompt。
- 异常场景：非法配置字段、模板缺失、路由策略无法匹配时返回预期错误。
