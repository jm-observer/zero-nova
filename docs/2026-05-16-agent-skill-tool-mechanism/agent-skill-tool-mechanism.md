# Agent Skill Tool 机制梳理

## 时间

- 创建时间：2026-05-16
- 最后更新：2026-05-16

## 项目现状

当前 `zero-nova` 的 Agent、Skill、Tool 机制由三条主要链路共同组成：

1. **Agent 配置与描述符链路**
   - `gateway.agents` 在 `.nova/config.toml` 中配置
   - `AgentSpec` 在 `crates/nova-agent-config/src/models.rs` 定义
   - runtime 启动时通过 `crates/nova-agent-loader/src/bootstrap.rs` 构建 `AgentRegistry`
   - 每个 agent 的 `prompt_file`、`tool_whitelist`、`enable_project_developer_prompt` 都在此阶段固化

2. **Skill 装载与路由链路**
   - skills 从 `.nova/skills/` 读取，经 `crates/nova-skill-loader` 解析
   - 解析结果通过 `crates/nova-agent-loader/src/skill_adapter.rs` 转成 `nova_agent::skill::SkillPackage`
   - `SkillRegistry` 负责 skill 索引、显式触发匹配、tool policy 推导
   - 当前 skill 激活只支持规则路由，核心入口在 `crates/nova-agent/src/agent/runtime.rs`

3. **Tool 注册与可见性链路**
   - 根 runtime 在 `crates/nova-agent-loader/src/bootstrap.rs` 中通过 `register_builtin_tools_with_services()` 注册工具
   - 子 agent runtime 在 `crates/nova-agent/src/tool/builtin/agent.rs` 中通过 `register_builtin_tools()` 按 `spec.tool_whitelist` 构建独立工具集
   - 每轮实际可见工具还会被 `prepare_turn()` 基于 active skill 再裁剪一遍

当前实现的关键特点：

- **Agent 差异主要来自 prompt 和项目级提示词加载，而不是 skill 注入模式**
- **Skill 注入是全局 prompt 构建行为，不是 per-agent 独立配置**
- **Tool 注册与 Tool 可见性是两个阶段，存在“已注册但本轮不可见”和“skill 白名单声明了但未真正出现在本轮工具列表”的分离现象**

## 整体目标

本次梳理的目标不是修改行为，而是明确当前机制的真实分层、职责边界和耦合点，为后续重构提供统一基线，重点回答：

1. Agent 的 prompt、skill、tool 差异分别由哪里决定
2. 主 agent 与子 agent 的 tool/runtime 构建路径有何区别
3. active skill 如何影响 prompt 注入与工具可见性
4. 当前设计中哪些点适合保留，哪些点应在重构时拆开

## Plan 拆分

| Plan | 描述 | 依赖 | 顺序 | 状态 |
|------|------|------|------|------|
| Plan 1 | 梳理 Agent / Skill / Tool 的配置、构建与运行时数据流，并沉淀重构关注点 | 无 | 1 | 已完成 |

## 风险与待定项

- `ToolPolicy::AllowListWithDeferred` 的语义在 policy 层、tool 注册层、turn 可见性层尚未完全对齐，后续重构需优先统一
- `/orchestrator` 这类显式 skill 触发目前依赖运行时 skill 路由与工具集可见性同时成立，稳定性不足
- `enable_project_developer_prompt` 只影响项目级开发提示词加载，不应继续和 skill / tool 机制混淆
