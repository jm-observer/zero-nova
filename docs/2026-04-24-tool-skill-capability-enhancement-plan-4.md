# 2026-04-24 tool-skill-capability-enhancement-plan-4

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 4：CLI / Gateway / DeskApp 集成、观测与评测 |
| 前置依赖 | Plan 2、Plan 3 |
| 本次目标 | 将 skill/tool 能力系统接入 CLI、gateway、deskapp 的真实链路，补齐事件协议、调试可视化、示例配置和回归测试，让该系统具备交付与持续迭代能力。 |
| 涉及文件 | `crates/nova-cli/src/main.rs`、`crates/nova-app/src/bootstrap.rs`、`crates/nova-app/src/types.rs`、`crates/nova-gateway-core/src/bridge.rs`、`deskapp/src` 相关状态展示模块、`.nova/README.md`、`.nova/examples/interaction-samples.json`、`.nova/examples/workflow-e2e.json`、`docs/todo/2026-04-24-claude-code-usage-analysis.md` |

## 详细设计

### 1. CLI 集成

CLI 至少补齐以下能力：

- `/skills`：列出当前可用 skill 与 active skill；
- `/skill <id>`：手动激活某个 skill，便于调试；
- `/exit-skill`：退出当前 skill；
- `/prompt-sections`：查看当前轮实际组装的 prompt sections；
- `/tasks`：查看当前 session 的 task 状态。

这样可以显式验证路由、能力策略和 prompt 组装结果，而不是继续靠日志猜测。

### 2. gateway / app 事件映射

桥接层需要把新增事件映射到前端可消费协议：

- skill activated / switched / exited
- tool unlocked
- capability policy changed
- task lifecycle

事件协议需要保持扁平、稳定，避免把内部结构直接暴露给前端。

### 3. deskapp 展示

桌面端建议至少增加两个轻量展示面：

1. 会话头部或侧栏显示：
   - 当前 active skill
   - 当前 agent
   - 当前能力模式概览
2. 进度面板显示：
   - task 列表
   - 最近一次解锁的 tool
   - 关键 skill 切换记录

第一阶段只做可读性展示，不做复杂交互编排器。

### 4. 示例与评测资产

在现有 `.nova/examples/` 基础上增加：

- `skill-routing-samples.json`
- `tool-unlock-samples.json`
- `workflow-skill-e2e.json`

用于覆盖：

- 路由命中；
- sticky skill；
- skill 切换摘要；
- ToolSearch 解锁；
- Task 编排。

同时更新 `.nova/README.md`，明确：

- skill 包推荐结构；
- tool / skill 能力关系；
- CLI 调试命令；
- 配置样例。

### 5. 回归测试策略

本 plan 不新增复杂评测框架，优先使用：

- `nova-core` 单元测试：数据结构、策略计算、prompt section；
- `nova-cli` 集成测试：命令与事件输出；
- `nova-app` / `gateway-core` 协议映射测试；
- 示例驱动测试：读取 `.nova/examples/*.json` 验证路由与事件流。

## 测试案例

1. 正常路径：CLI 能列出 skills、切换 active skill、退出 skill，并看到对应事件。
2. 正常路径：gateway 将 `SkillActivated`、`ToolUnlocked`、`TaskStatusChanged` 正确桥接到 app 事件。
3. 正常路径：deskapp 能展示当前 active skill 和任务进度，不因事件缺字段而崩溃。
4. 边界条件：无 skills 目录时，CLI / gateway 仍能正常运行，仅禁用相关展示。
5. 边界条件：skill 路由被关闭时，系统仍能通过手动 `/skill` 进入调试模式。
6. 异常场景：前端收到未知 skill/tool 事件类型时安全忽略并记录日志。
7. 回归场景：基于示例文件跑完整 workflow，验证 skill 激活、task 更新、tool 解锁顺序符合预期。
