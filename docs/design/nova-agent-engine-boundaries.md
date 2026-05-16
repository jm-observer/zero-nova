# Nova Agent Engine Boundaries

## 范围

本文档描述 `crates/nova-agent` 当前与外部 crate 交互时的核心边界，重点覆盖：

- `AgentRuntime`
- `ToolRegistry`
- `AgentApplication`
- `OrchestratorEngine`

## AgentRuntime

`AgentRuntime` 负责：

- 根据当前上下文准备 turn
- 组装统一工具定义集合与 turn 元数据
- 驱动 provider 流式响应与工具执行循环

`AgentRuntime` 持有 `ToolRegistry`，但不拥有工具装配逻辑；工具装配由 `nova-agent-loader` 或子代理装配路径在异步上下文中完成。

当前稳定边界：

- `prepare_turn()` 可以解析 active skill，并将其作为 prompt / metadata 语义输入。
- `prepare_turn()` 不再因为 active skill 或 agent 身份不同而裁剪工具集合。
- turn 级能力策略已收敛为轻量元数据，主要用于调试、追踪和策略表达，不再承担工具开关职责。

## ToolRegistry

`ToolRegistry` 是运行时内的工具索引，当前基线如下：

- 对外公共接口只提供异步方法。
- 工具注册、工具定义读取、turn tool view 读取都必须通过 `await` 调用。
- registry 只维护单一 loaded 工具集合，不再区分 deferred tool、按需 load 或 ToolSearch 注入入口。
- 根 runtime 与子 agent runtime 使用同一套工具注册规则，不再存在 agent 级工具白名单分叉。

这样做的目的是消除在 Tokio 运行时内对同步自旋锁辅助函数的依赖，避免出现同步与异步两套公共调用约定。

## AgentApplication

`AgentApplication` 是 `nova-agent` 对网关和服务端暴露的总门面。当前与语音能力相关的稳定行为为：

- `voice_capabilities` 可以报告当前配置层声明的能力状态。
- `voice_transcribe` 与 `voice_tts` 在真实语音服务未接线前，必须返回显式错误 `voice not implemented`。
- 禁止在未实现路径使用 `todo!()` 或其他 panic 行为，因为网关会直接分发真实请求到该接口。

## OrchestratorEngine

`OrchestratorEngine` 负责解析 orchestration plan、调度 stage 执行、汇总子代理结果并驱动 review 阶段。当前稳定边界如下：

- engine 不再直接依赖 `AgentTool` 具体类型，而是依赖 `SubAgentExecutor` trait。
- `SubAgentExecutor` 必须同时提供子代理执行入口、catalog agent id 集合以及默认 agent id。
- 生产环境由 `AgentTool` 实现该 trait；测试环境允许注入 mock executor，避免 orchestration 测试依赖真实 LLM 或完整子代理运行时。
- `OrchestrateTaskTool` 仍作为外部入口负责构造 engine，现有外部可观测行为保持不变。
- 仅当所有已执行 stage 成功且 orchestration 未被取消时，engine 才会进入 review 阶段；stage 失败时必须直接结束 orchestration，并返回失败或依赖阻塞摘要。

补充说明：

- 编排能力属于统一共享能力集合的一部分，不再依赖某个 agent 的专属工具可见性。
- 是否使用编排由 prompt 与任务结构共同决定，而不是由注册层强制隔离。
