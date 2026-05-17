# Plan 3: 会话清理钩子与测试 / 文档收尾

## 前置依赖

Plan 1、Plan 2。

## 任务目标

session 删除时释放其激活集合，避免按 session 累积；补端到端测试；更新长期设计资产与 ADR。`cargo clippy/fmt/test` 全绿。

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/app/application.rs`（`delete_session` 调用清理）
  - `docs/design/system-overview.md`（工具系统章节）
  - `docs/adr/2026-05-17-session-scoped-tool-activation.md`（新增）
  - `docs/2026-05-17-session-scoped-tool-activation/session-scoped-tool-activation.md`（更新 Plan 状态）
- 允许修改：`crates/nova-agent/tests/external_tool_integration.rs`（端到端测试）
- 禁止修改：Plan 1/2 已定型逻辑

## Agent 执行步骤

1. 必须在 `application.rs::delete_session` 成功删除 session 后调用 `registry.clear_session_activations(session_id)`；需确认 `AgentRuntime` 暴露可达 registry 的访问点（如无，必须新增 `pub(crate) async fn clear_session_tool_activations(&self, session_id: &str)` 转调 `self.tools.clear_session_activations`）。
2. 必须在 `external_tool_integration.rs` 新增端到端测试：两个不同 session 分别激活/未激活同一外部工具，断言视图隔离；删除 session 后断言释放。
3. 必须更新 `docs/design/system-overview.md`：在工具系统章节描述 always-on / deferred / 会话激活三层模型与隔离语义。
4. 必须新增 `docs/adr/2026-05-17-session-scoped-tool-activation.md`：记录从"全局解析"到"会话级隔离"的决策、动机、影响、被取代的旧行为。
5. 必须把总览文档三个 Plan 状态更新为「已完成」，并执行 commit。

## 行为规则

| 输入 | 处理路径 | 期望输出 |
|------|----------|----------|
| `delete_session("s1")`，s1 曾激活 T | 删除 session → `clear_session_activations("s1")` | s1 激活集合释放；后续无 s1 残留 |
| 删除 s1 不影响 s2 | 仅清 s1 | s2 激活集合不变 |
| 删除不存在 session | `clear_session_activations` no-op | 不 panic、不报错 |

## 禁止事项

- 禁止改动 Plan 1/2 的语义与签名。
- 禁止在 `delete_session` 失败路径调用清理（仅成功删除后清理）。
- 禁止跳过修复循环直接 commit。

## 测试要求

文件：`crates/nova-agent/tests/external_tool_integration.rs`

| 测试名 | 输入 | 期望断言 |
|--------|------|----------|
| `activation_isolated_across_sessions_e2e` | 加载外部工具目录；s1 激活某工具，s2 不激活 | s1 turn view `loaded` 含该工具；s2 `deferred` 含该工具 |
| `delete_session_releases_activations_e2e` | s1 激活后调用清理 | s1 turn view 中该工具回到 `deferred` |

验证命令：
```
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `delete_session` 释放会话激活集合
- [ ] 2 个端到端测试通过
- [ ] `docs/design/system-overview.md` 已更新工具系统章节
- [ ] `docs/adr/2026-05-17-session-scoped-tool-activation.md` 已新增
- [ ] 总览文档三 Plan 状态置为「已完成」
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
