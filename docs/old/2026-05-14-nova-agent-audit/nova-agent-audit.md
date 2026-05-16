# nova-agent 代码审计报告

## 时间

2026-05-14

## 项目现状

`nova-agent` 是项目核心 crate，承载了几乎所有业务逻辑：Agent 运行时、工具系统、Prompt 构建、Skill 路由、LLM Provider 对接、会话管理、编排器等。当前代码库存在若干结构性问题与过度设计，亟需整理。

**代码体量统计：**

| 模块 | 最大文件 | 大小 |
|------|---------|------|
| `prompt/` | `mod.rs` | 90 KB / 2512 行 |
| `conversation/service/` | `mod.rs` | 50 KB |
| `conversation/repository/` | `mod.rs` | 38 KB |
| `tool/` | `registry.rs` | 43 KB |
| `app/` | `agent_workspace_service.rs` | 35 KB |
| `config/` | `models.rs` | 23 KB |

## 整体目标

识别并分类现有架构中的不合理设计，为后续重构工作提供明确的优先级排序。

## 问题分类与 Plan 拆分

| Plan | 说明 | 状态 |
|------|------|------|
| Plan 1 | 🔴 严重问题（超规模文件） | 待开始 |
| Plan 2 | 🟡 过度设计（同步/异步双写、双锁、双路径等） | 待开始 |
| Plan 3 | 🟡 结构性问题（职责过宽、重复定义等） | 待开始 |

---

## 风险与待定项

- `prompt/mod.rs` 拆分已启动但未完成（`builder.rs`、`context.rs` 等子模块存在但旧代码未清理）
- `agent/stream_bridge.rs`、`agent/turn_executor.rs` 是空占位文件，Plan 4 拆分从未实施
