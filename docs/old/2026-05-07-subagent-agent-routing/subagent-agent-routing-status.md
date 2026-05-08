# 子 Agent Agent Routing 当前状态

- **时间**：2026-05-07
- **对应设计**：`subagent-agent-routing.md`
- **当前状态**：Plan 1 / Plan 2 已实现并已提交

---

## 本次已完成内容

### 配置与提示词

1. 在 `.nova/config.toml` 中新增 `developer` Agent 注册
2. 新增 `.nova/prompts/agent-developer.md`
3. 保持 `nova` 为默认通用 Agent，不改变顶级默认入口

### 编排与运行时

1. 编排计划中的 `subagent_type` 现在用于选择实际执行 Agent
2. 开发类子任务预期输出 `subagent_type=developer`
3. 非开发类或不确定任务预期输出 `subagent_type=nova`
4. `subagent_type` 缺失时，解析层默认补 `nova`
5. `subagent_type` 非法时，运行时 warning 后回退 `nova`

### 文档与提交

1. Plan 1 / Plan 2 设计文档状态已更新为“已完成”
2. 已提交 commit：`4e50667`
3. commit message：`Route coding subtasks through a dedicated agent`

---

## 当前实现口径

为兼容现有协议与前端事件字段，当前保留双字段语义：

| 字段 | 当前语义 |
|---|---|
| `agent_id` | 编排 Plan 内唯一实例标识 |
| `subagent_type` | 实际执行 Agent 选择，当前为 `nova` 或 `developer` |

这套语义已经能支撑首版功能，但命名不够直观，后续若继续演进，优先考虑统一协议字段。

---

## 已完成验证

本轮实现已通过：

1. `cargo clippy --workspace -- -D warnings`
2. `cargo fmt --all --check`
3. `cargo test --workspace`

---

## 当前已知问题

### 1. 字段语义仍有历史包袱

`agent_id` 与 `subagent_type` 分别承担“实例标识”和“执行 Agent 选择”，阅读成本偏高。

### 2. 角色仍然只有两类

当前只支持：

- `nova`
- `developer`

尚未引入 `reviewer` 等更细粒度子 Agent。

### 3. 仍有与本次无关的工作树改动

当前仓库中还存在未提交改动，但不属于这次实现范围：

1. `crates/nova-agent/src/orchestrator/scheduler.rs`
2. `crates/nova-agent/src/skill.rs`
3. `crates/nova-server/src/bin/nova_gateway_ws.rs`
4. `docs/test/2026-05-06-multi-agent-orchestration/test-05-cancel-inflight.md`

后续继续开发时，需要避免误把这些改动混入新的提交。

---

## 建议下一步

如果继续推进，建议优先级如下：

1. 统一协议与前后端命名，消除 `agent_id` / `subagent_type` 的双重语义
2. 视实际使用效果，再决定是否新增 `reviewer` 子 Agent
3. 如需前端可视化优化，再补显示“实际执行 Agent 类型”的展示字段
