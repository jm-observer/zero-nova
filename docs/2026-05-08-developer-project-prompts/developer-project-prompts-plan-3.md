# Plan 3: 会话链路接入与验证

## 前置依赖

Plan 1、Plan 2

## 本次目标

将开发项目提示词加载能力接入实际会话执行链路，并保证 prompt reload 与主要测试路径行为一致。

## 涉及文件

1. `crates/nova-agent/src/app/conversation_service.rs`
2. `crates/nova-agent/src/app/agent_workspace_service.rs`
3. `crates/nova-agent/src/app/bootstrap.rs`
4. 相关集成测试与 prompt preview 测试

## 详细设计

### 1. 会话执行期接入点

主接入点放在 `ConversationService::execute_agent_turn`：

1. 先读取当前 session 的 `project_dir`
2. 根据当前 agent 配置判断 `enable_project_developer_prompt`
3. 若关闭，跳过开发项目提示词读取
4. 若开启，调用 Plan 2 的加载函数
5. 将结果写入 `PromptConfig`

这样能保证每轮对话都以当前 session 绑定的项目根目录为准。

### 2. reload 链路同步

`AgentWorkspaceService::reload_session_system_prompt` 需要复用同一判断逻辑：

1. 取 session 当前 `project_dir`
2. 判断 agent 开关
3. 若开启则重新加载开发项目提示词
4. 重新构建 `PromptConfig`
5. 更新 prompt preview 与版本指纹

否则会出现“实时会话 prompt”与“reload 后 prompt”不一致的问题。

### 3. bootstrap 阶段处理

`bootstrap` 阶段不应主动读取开发项目提示词文件，因为此时没有可靠的 session `project_dir`。因此这里只需：

1. 保留配置到运行时对象中
2. 不在启动时进行项目根文件 I/O

这避免把 session 级动态上下文错误地固化进启动期 agent 描述。

### 4. CLI 路径

CLI 可以采用保守接入：

1. 若未来 CLI 明确传入项目目录，则复用同一加载逻辑
2. 当前若无稳定 `project_dir`，则默认不注入

首版不需要为 CLI 增加额外参数，只要保证不会产生与主链路相冲突的半成品行为。

### 5. 观测与调试

为了方便后续排查，建议：

1. 在命中至少一个文件时记录 `info!`
2. 记录命中文件数量和相对路径列表
3. 在 prompt preview 中保留完整 `Developer Project Instructions` section

这样既不重复打印全部内容，也能为问题定位保留可观测性。

## 测试案例

1. agent 开关关闭时，即使项目根存在文件也不注入
2. agent 开关开启且存在项目根文件时，会话 prompt 中包含开发项目提示词 section
3. reload session system prompt 时能够重新读取文件内容
4. 更换 session `project_dir` 后，下一轮 prompt 会读取新项目根目录内容
5. 未设置 `project_dir` 时不会注入开发项目提示词且不会报错
