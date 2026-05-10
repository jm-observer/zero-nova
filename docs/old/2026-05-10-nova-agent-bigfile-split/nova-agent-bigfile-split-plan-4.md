# Plan 4: agent.rs、skill.rs、tool.rs 拆分设计

## 前置依赖
- Plan 1

## 本次目标
- 拆分运行时核心（`agent.rs`）、技能系统（`skill.rs`）与工具注册执行（`tool.rs`）三大域。
- 保持现有对外 API 稳定，优先以模块化重排降低文件复杂度。
- 将策略决策、数据模型、执行编排、校验逻辑解耦，减少跨域互相调用。

## 涉及文件
- `crates/nova-agent/src/agent.rs`
- `crates/nova-agent/src/agent/mod.rs`（新增）
- `crates/nova-agent/src/agent/runtime.rs`（新增）
- `crates/nova-agent/src/agent/turn_executor.rs`（新增）
- `crates/nova-agent/src/agent/stream_bridge.rs`（新增）
- `crates/nova-agent/src/skill.rs`
- `crates/nova-agent/src/skill/mod.rs`（新增）
- `crates/nova-agent/src/skill/model.rs`（新增）
- `crates/nova-agent/src/skill/registry.rs`（新增）
- `crates/nova-agent/src/skill/policy.rs`（新增）
- `crates/nova-agent/src/tool.rs`
- `crates/nova-agent/src/tool/mod.rs`（若需要调整）
- `crates/nova-agent/src/tool/registry.rs`（新增）
- `crates/nova-agent/src/tool/schema_validation.rs`（新增）
- `crates/nova-agent/src/tool/path_preprocess.rs`（新增）

## 详细设计
1. `agent` 域拆分
- `runtime.rs`：`AgentRuntime` 生命周期、依赖注入、主入口。
- `turn_executor.rs`：单轮对话执行编排、事件流协调、工具调用次序。
- `stream_bridge.rs`：provider 流式事件到内部事件的映射。

2. `skill` 域拆分
- `model.rs`：`Skill`、`SkillPackage`、策略相关纯数据结构。
- `registry.rs`：技能发现、加载、缓存、查询。
- `policy.rs`：能力策略合并、优先级决策、冲突处理。

3. `tool` 域拆分
- `registry.rs`：`ToolRegistry`、deferred tool 管理、turn 视图。
- `schema_validation.rs`：入参 schema 校验与错误格式。
- `path_preprocess.rs`：文件工具路径预处理与 project_dir 约束。

4. 迁移规则
- 首先只做“函数搬迁 + use 调整 + re-export”，不改行为。
- 对超长函数先提取私有 helper，减少单函数复杂度。
- 所有新增模块默认私有，仅通过父模块选择性导出。

## 测试案例
- 正常路径：agent 单轮执行、tool execute、skill 路由全链路回归。
- 边界条件：legacy tool name 映射、deferred tool 加载、policy 空配置。
- 异常场景：schema 不匹配、路径非法、找不到 tool/skill。
