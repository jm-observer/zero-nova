# Plan 4: 前端展示与回归验证

- **前置依赖**：Plan 1、Plan 3
- **状态**：待开始

---

## 本次目标

修正前端编排 UI 的字段消费和状态更新逻辑，并补齐后端、前端两侧的回归测试，确保修复后的能力不会再次被文档漂移或字段漂移打坏。

**可验证标准：**
- `orchestration_plan` 能渲染出 Plan 卡片
- `sub_agent_spawn` / `sub_agent_complete` 能正确更新卡片状态
- 多个 stage 和多个 plan 的状态不会互相串线
- 新增测试覆盖协议、事件路由和 UI 状态变化

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `deskapp/src/services/chat-service.ts` | 修改 | 确认 `ProgressEvent.args` 原样转发 |
| `deskapp/src/ui/orchestration-view.ts` | 修改 | 字段读取、Plan/Agent 定位和状态更新 |
| `deskapp/src/styles/main/orchestration.css` | 复核 | 仅在状态 class 变化需要时微调 |
| `deskapp/src/__tests__/...` | 新增/修改 | 前端事件消费与渲染测试 |
| `crates/nova-agent/tests/...` | 新增 | 编排器与工具入口测试 |
| `crates/nova-protocol/tests/...` | 新增/修改 | 协议 roundtrip 测试 |

---

## 详细设计

### 1. 修正前端字段读取与路由

`chat-service.ts` 已经按 `event.type` 路由编排事件，但前提是 gateway 真的发出这些类型。Plan 3 修复后，这里只需要确保：

- 不对 `args` 做 snake_case 改写
- `agent_log` 保留 `agentId`、`stageId`、`planId` 维度

### 2. 提升状态定位稳定性

当前 `OrchestrationView` 的 `findAgent()` 在全局按 `agentId` 查找；若不同 plan 都使用 `a1`，状态会串线。

修复建议：

- 内部状态查找优先用 `planId + agentId`
- 事件 args 若可提供 `planId`，前端必须利用该字段
- 至少在 `PlanState` 中保留 agent 反向索引，避免全表扫描

### 3. 补齐端到端断言

测试分层建议：

- 协议层：字段名、roundtrip
- 工具/编排器层：事件序列与失败语义
- 前端服务层：`chat.progress` 到 EventBus 的映射
- UI 层：Plan 渲染、Agent 状态切换、失败显示

### 4. 修复完成后的检查循环

Plan 4 完成后必须执行完整修复流程：

1. `cargo clippy --workspace -- -D warnings`
2. `cargo fmt --all --check`
3. `cargo test --workspace`
4. 若 deskapp 独立有前端测试命令，再补充其测试命令

若 `schema-types.ts` 由测试生成，需把生成产物纳入检查范围，避免本地通过、提交后不一致。

---

## 测试案例

### T4-01：Plan 首次渲染
- 输入：`orchestration_plan` 事件，含 2 个 stage
- 预期：UI 生成 Plan 区块，进度显示 `0 / N`

### T4-02：Agent 运行中状态
- 输入：`sub_agent_spawn`
- 预期：对应卡片变为 `running`，Plan 状态从 `planning` 进入 `running`

### T4-03：Agent 完成状态
- 输入：`sub_agent_complete`
- 预期：卡片状态更新、摘要显示、进度条增加

### T4-04：失败可视化
- 输入：失败态 `sub_agent_complete`
- 预期：卡片标红，失败摘要或错误信息可见

### T4-05：多 Plan 隔离
- 输入：同 session 下两个 `planId`，都含 `agentId = "a1"`
- 预期：两个 Plan 的卡片互不串线

### T4-06：完整回归
- 输入：调用 `OrchestrateTask` 执行一条最小合法 plan
- 预期：后端事件、gateway 转发、前端渲染和测试断言全部通过
