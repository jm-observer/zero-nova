# Skill Loader Split

## 时间

- 创建日期：2026-05-14
- 最后更新：2026-05-14

## 项目现状

`nova-agent` 的 `SkillRegistry` 同时承担 skill 存储、查询、目录发现、文件读取与格式解析职责，导致 agent 核心逻辑与可复用加载能力耦合。当前支持 `skill.toml` 与兼容格式 `SKILL.md`，运行时主要通过异步加载入口初始化 registry。

## 整体目标

在保留 agent 完整行为的前提下，将 skill 目录发现、单技能读取与格式解析提取到独立 workspace crate，让 `nova-agent` 只负责 registry 状态维护、路由、策略和 prompt 生成。

## Plan 拆分

| Plan | 描述 | 依赖 | 执行顺序 | 完成状态 |
| --- | --- | --- | --- | --- |
| Plan 1 | 新增 `nova-skill-loader` crate，承载加载模型、目录发现与解析逻辑 | 无 | 1 | 已完成 |
| Plan 2 | 将 `nova-agent` 的 registry 加载入口改为调用 loader，并保留现有公开 API | Plan 1 | 2 | 已完成 |
| Plan 3 | 执行 clippy、fmt、test 修复流程，确保拆分不改变行为 | Plan 2 | 3 | 已完成 |

## 风险与待定项

- `skill.toml` 的 `tool_policy` 当前沿用既有解析语义；若未来需要支持更结构化的 TOML 表，需要单独补充兼容设计。
- 本次只拆分 skill 加载能力，不改动路由、策略裁剪和 prompt 注入逻辑，避免扩大变更范围。
