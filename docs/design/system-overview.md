# System Overview

## 概要

当前 `zero-nova` workspace 由以下几类模块组成：

- `crates/nova-agent`：核心 Agent 运行时、工具系统、会话与应用层门面。
- `crates/nova-agent-loader`：负责从配置装配 `nova-agent` 的运行时与应用实例。
- `crates/nova-gateway-core` / `crates/nova-server`：将应用层能力暴露给网关与传输层。
- `crates/nova-cli`：本地 CLI 调试入口。

## 模块索引

- `docs/design/nova-agent-engine-boundaries.md`
  说明 `nova-agent` 中运行时、工具注册表、应用层门面、orchestrator 与网关边界的稳定职责。

## 能力统一基线

当前 Agent / Skill / Tool 机制的稳定基线如下：

- 所有 agent 共享同一套 skill registry。
- 所有 agent 共享同一套工具注册集合，主 agent 与子 agent 不再通过配置获得不同工具集。
- turn 级 active skill 仍可影响 prompt 语义与工作流提示，但不再裁剪工具可见性。
- agent prompt 的职责是表达角色偏好，而不是隔离能力。

这意味着系统中的“能力”与“职责”已拆分：

- 能力由统一注册和统一 skill 装载提供。
- 职责由 agent prompt、skill prompt 和工作流规则表达。

## 当前约束

- 工具系统的公共注册与读取接口统一为异步方法，调用方必须在异步上下文中完成工具注册、列举和工具视图读取。
- orchestration 执行层通过 `SubAgentExecutor` trait 与具体子代理执行器解耦，便于注入 mock 并保持外部入口稳定。
- 应用层语音能力在未接入真实 STT/TTS 服务前，允许暴露 capability 状态，但执行请求必须返回显式错误，不能 panic。
