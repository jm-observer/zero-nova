# 外部 Tool 注入

- 创建时间：2026-05-17
- 状态：设计中

## 项目现状

`nova-agent` 的 tool 系统目前仅支持内置 tool（硬编码在 `builtin/` 中），无法从外部加载。
Registry 已有 deferred 机制的数据结构（`DeferredToolEntry`、`DeferredToolRepresentation`、`resolve_deferred`），
但当前 `register_deferred` 被简化为立即加载，deferred 列表实际为空。

## 整体目标

支持从目录加载外部 tool 定义文件（兼容 mcp-tool-generator 格式），注册为 deferred tool，
通过已有的 `tool_search` 机制按需激活。

## Plan 拆分

| Plan | 标题 | 依赖 | 状态 |
|------|------|------|------|
| 1 | Tool 定义文件解析 | 无 | 已完成 |
| 2 | ExternalCommandTool 实现 | Plan 1 | 已完成 |
| 3 | 目录扫描与 Deferred 注册 | Plan 1, 2 | 已完成 |
| 4 | 集成测试 | Plan 1, 2, 3 | 已完成 |

## 风险与待定项

- **Tool 激活机制**（上轮用过的下轮继续带 vs 每轮无状态）：标记为 TODO，后续根据使用体验决定。
- **安全隔离**：外部 command 的超时、输出截断、环境变量白名单，本次实现基础版，后续迭代加强。
