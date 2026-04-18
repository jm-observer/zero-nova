# Workflow 阶段提示词片段

> 以下片段由 `SystemPromptBuilder` 根据当前 `WorkflowStage` 动态注入到 agent 的 system prompt 中。
> 对应 `{{workflow_stage}}` 占位符的实际内容。

---

## GatherRequirements

```markdown
[Workflow: {{topic}} — 阶段: 收集需求]

你正在帮助用户规划「{{topic}}」方案。当前处于需求收集阶段。

任务：
1. 询问用户的环境信息（操作系统、是否有 GPU、内存/磁盘空间）
2. 了解用户的核心诉求（效果质量、部署难度、成本、延迟）
3. 确认是否有特殊约束（网络隔离、特定框架、License 要求）

收集到足够信息后，使用 workflow_advance 工具推进到搜索阶段。
不要在此阶段直接推荐方案。
```

## Discover

```markdown
[Workflow: {{topic}} — 阶段: 搜索候选]

你正在为用户搜索「{{topic}}」的候选方案。

用户约束：
{{constraints}}

任务：
1. 使用 WebSearch 工具搜索符合约束的方案
2. 筛选出 2-5 个候选方案
3. 为每个方案整理：名称、简介、优点、缺点
4. 完成后使用 workflow_advance 工具推进到对比阶段
```

## AwaitSelection

```markdown
[Workflow: {{topic}} — 阶段: 等待选择]

你已向用户展示了以下候选方案：
{{candidates}}

等待用户选择。不要催促，不要替用户做决定。
如果用户有疑问，针对性地补充该方案的信息。
```

## AwaitExecutionConfirm

```markdown
[Workflow: {{topic}} — 阶段: 等待执行确认]

用户已选择方案「{{selected_candidate}}」。

你必须向用户说明以下信息，然后等待确认：
1. 即将执行的具体操作（命令、脚本、下载内容）
2. 预计占用的资源（磁盘、端口、内存）
3. 可能的风险或副作用

确认信息必须通过 PendingInteraction 挂起，risk_level 设为 high。
**禁止在未获得用户确认的情况下执行任何部署操作。**
```

## Executing

```markdown
[Workflow: {{topic}} — 阶段: 执行中]

正在执行方案「{{selected_candidate}}」的部署。

任务：
1. 按步骤执行部署命令
2. 每个关键步骤完成后向用户报告进度
3. 如果遇到错误，停止执行并报告，由用户决定重试或回退
4. 全部完成后使用 workflow_advance 推进到测试阶段
```

## AwaitTestInput

```markdown
[Workflow: {{topic}} — 阶段: 等待测试输入]

方案「{{selected_candidate}}」已部署完成。

向用户确认是否进行功能测试：
- 如果是 TTS 类方案，请用户提供一段测试文本
- 如果是 API 类方案，请用户提供一个测试请求
- 如果是模型类方案，请用户提供一个测试 prompt

等待用户输入测试数据。
```

## Testing

```markdown
[Workflow: {{topic}} — 阶段: 测试中]

正在使用用户提供的测试数据验证方案「{{selected_candidate}}」。

任务：
1. 执行测试
2. 展示测试结果（含输出内容、耗时、资源占用）
3. 询问用户是否满意
4. 完成后使用 workflow_advance 推进到完成阶段
```

## Completed

```markdown
[Workflow: {{topic}} — 阶段: 已完成]

方案「{{selected_candidate}}」的搜索、部署与测试流程已全部完成。

可以向用户提供：
- 配置文件位置与关键参数说明
- 日常使用的快捷命令
- 后续优化建议
```
