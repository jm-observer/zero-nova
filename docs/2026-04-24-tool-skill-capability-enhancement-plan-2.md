# 2026-04-24 tool-skill-capability-enhancement-plan-2

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 2：Skill 路由、激活态与 Prompt 组装 |
| 前置依赖 | Plan 1 |
| 本次目标 | 把 skill 从“静态提示词集合”升级为“会话级可激活能力”，建立路由、切换、sticky、退出和历史切片机制，并在 `run_turn` 前生成真正的 `TurnContext`。 |
| 涉及文件 | `crates/nova-core/src/agent.rs`、`crates/nova-core/src/prompt.rs`、`crates/nova-core/src/skill.rs`、`crates/nova-core/src/event.rs`、`crates/nova-app/src/bootstrap.rs`、`crates/nova-cli/src/main.rs`、`.nova/prompts/turn-router.md`、`.nova/prompts/workflow-stages.md` |

## 详细设计

### 1. session 级 active skill 状态

新增 `ActiveSkillState`，由 session 持有：

- `active_skill_id`
- `sticky`
- `entered_at`
- `last_routed_at`
- `summary_of_previous_segment`

该状态不应该放进 `SkillRegistry`，而应放在 session/runtime 层，因为它代表运行时行为。

### 2. skill 路由流程

每轮请求前执行 `SkillRouter::route()`，返回：

- `KeepCurrent`
- `Activate(skill_id)`
- `Deactivate`
- `NoSkill`

路由优先级：

1. 若 active skill 且 sticky，则默认 `KeepCurrent`；
2. 若用户显式输入退出指令，如 `/exit-skill`、`/reset-skill`，则 `Deactivate`；
3. 否则根据用户消息和可选候选 skill 描述做路由；
4. 无高置信结果则返回 `NoSkill`。

第一阶段不强制引入新模型，可复用现有主模型配置或已有 turn-router prompt，通过单次低 token 路由调用实现。

### 3. skill 切换与历史切片

当前实现将所有消息平铺在一个 `history` 里。目标上，skill 切换时执行：

1. 结束当前 active segment；
2. 将旧 segment 规约为摘要对象：
   - 用户目标
   - 已做决策
   - 未完成事项
   - 关键路径引用
3. 新建新的 active segment；
4. 后续 prompt 中只保留：
   - 全局摘要；
   - 当前 skill 摘要；
   - 当前 active segment 原始消息。

这样可以控制 token，且不破坏多轮工作流连续性。

### 4. TurnContext 构建

在 `AgentRuntime::run_turn` 调用前，引入显式的 turn preparation：

1. 决定 active skill；
2. 根据 active skill 生成 capability policy；
3. 生成 system prompt sections；
4. 过滤工具定义；
5. 裁剪历史；
6. 构造最终 `TurnContext`。

运行时接入方式建议：

- 新增 `prepare_turn()`；
- `run_turn()` 只消费已经准备好的上下文；
- CLI / app / gateway 共用同一套准备逻辑。

### 5. 事件与可观测性

新增或明确以下事件：

- `SkillActivated`
- `SkillSwitched`
- `SkillExited`
- `SkillRouteEvaluated`

这些事件作用：

- CLI 能打印当前 skill 变化；
- gateway 能透出给桌面端；
- 评测工具能断言路由结果是否符合预期。

## 测试案例

1. 正常路径：无 active skill 时，用户输入匹配某个 skill，路由结果为 `Activate`，system prompt 含对应 skill section。
2. 正常路径：active sticky skill 存在时，普通后续消息保持 `KeepCurrent`。
3. 正常路径：用户显式退出后，skill 被清空，后续回到默认模式。
4. 边界条件：路由无匹配时，不激活 skill，但会话继续运行。
5. 边界条件：skill 从 A 切到 B 时，A 的历史被摘要，B 的 active segment 重新开始。
6. 异常场景：路由调用失败时，回退到 `NoSkill`，不能阻塞整轮对话。
7. 异常场景：skill prompt 缺失时，返回带路径信息的错误，而不是 silently fallback。
