# Plan 1: Agent / Skill / Tool 机制梳理

## 前置依赖

无

## 任务目标

产出一份可直接指导重构的机制说明，明确：

- Agent 配置如何进入运行时
- Skill 如何被加载、匹配、注入 prompt、影响工具可见性
- Tool 如何在根 runtime 与子 agent runtime 中注册
- 当前机制中的关键耦合点和重构切入点

## 执行范围

- 必须修改：
  - `docs/2026-05-16-agent-skill-tool-mechanism/agent-skill-tool-mechanism.md`
  - `docs/2026-05-16-agent-skill-tool-mechanism/agent-skill-tool-mechanism-plan-1.md`
- 允许读取：
  - `.nova/config.toml`
  - `.nova/prompts/`
  - `.nova/skills/`
  - `crates/nova-agent-config/`
  - `crates/nova-agent-loader/`
  - `crates/nova-agent/src/agent/`
  - `crates/nova-agent/src/prompt/`
  - `crates/nova-agent/src/skill/`
  - `crates/nova-agent/src/tool/`
- 禁止修改：
  - 生产代码
  - skill 内容
  - agent prompt 内容

## Agent 执行步骤

1. 读取 `.nova/config.toml` 中 `gateway.agents` 配置，记录 `prompt_file`、`tool_whitelist`、`enable_project_developer_prompt` 的职责
2. 读取 `crates/nova-agent-config/src/models.rs`，确认 `AgentSpec` 的实际字段定义
3. 读取 `crates/nova-agent-loader/src/bootstrap.rs`，梳理根 runtime、`SkillRegistry`、`AgentRegistry` 的构建顺序
4. 读取 `crates/nova-agent/src/prompt/builder.rs`，确认 skill prompt 注入点与 `SkillInjectionMode` 的行为
5. 读取 `crates/nova-agent/src/agent/runtime.rs`，梳理 `prepare_turn()`、`decide_active_skill()`、`filter_tool_definitions()` 的执行顺序
6. 读取 `crates/nova-agent/src/skill/registry/filter.rs`，梳理 `match_skill_by_input()` 与 `policy_from_skill()` 的规则
7. 读取 `crates/nova-agent/src/tool/builtin/mod.rs` 与 `crates/nova-agent/src/tool/builtin/agent.rs`，梳理根 agent 与子 agent 的 tool 注册差异
8. 在总览文档中写出三条主链路、关键耦合点、重构建议与待定项

## 目标数据结构 / 接口契约

本 Plan 不新增生产代码结构，仅记录现状契约：

```rust
pub struct AgentSpec {
    pub id: String,
    pub prompt_file: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub enable_project_developer_prompt: bool,
}
```

```rust
pub enum ToolPolicy {
    InheritAll,
    AllowList(Vec<String>),
    AllowListWithDeferred(Vec<String>),
}
```

```rust
pub async fn prepare_turn(
    &self,
    input: &str,
    current_history: Arc<Vec<Message>>,
    system_prompt: String,
) -> Result<TurnContext>
```

## 行为规则

| 输入 / 场景 | 处理路径 | 当前结果 |
|------|----------|----------|
| 配置 `gateway.agents[].prompt_file` | 启动时构建 agent descriptor | 不同 agent 使用不同基础 prompt |
| 配置 `gateway.agents[].enable_project_developer_prompt = true` | turn prompt loader / subagent prompt loader | 额外加载项目级开发说明 |
| 用户输入 `/skill-xxx` 或 `/<skill>` | `SkillRegistry::match_skill_by_input()` | 产生 active skill |
| active skill 命中 `AllowList` / `AllowListWithDeferred` | `policy_from_skill()` + `filter_tool_definitions()` | 当前轮工具被裁剪 |
| 子 agent 启动 | `AgentTool` 内部构建 sub runtime | 工具集受 `spec.tool_whitelist` 影响 |
| 技能注入 prompt | `SystemPromptBuilder::build_from_request()` | 按 `SkillInjectionMode` 注入 catalog / active / full |

## 禁止事项

- 不要修改运行时行为
- 不要新增依赖
- 不要混入修复 `/orchestrator` 的实现改动
- 不要在文档中假设不存在的运行时分支
- 不要把“prompt 差异”和“tool 可见性差异”混为同一个配置来源

## 测试要求

- 本 Plan 仅新增文档，不要求新增测试
- 不执行生产代码修改验证

## 完成条件

- [x] Agent 配置、Skill 装载、Tool 注册三条链路已分别说明
- [x] 已明确 `prompt_file`、`tool_whitelist`、`enable_project_developer_prompt` 的职责边界
- [x] 已明确 skill 注入与 tool 可见性分别发生在什么阶段
- [x] 已记录当前设计的关键耦合点和重构关注项
- [x] 已在 `docs/2026-05-16-agent-skill-tool-mechanism/` 下落盘文档
