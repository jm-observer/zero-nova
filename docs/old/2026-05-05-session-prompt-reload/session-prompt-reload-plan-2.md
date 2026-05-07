# Plan 2: 后端 Session 重载链路实现

## 前置依赖
Plan 1

## 本次目标

实现“当前 Session 重新读取配置并重拼接 system prompt”的后端链路，保证线程安全、并发语义稳定、失败可回滚。

## 涉及文件

- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/control.rs`
- `crates/nova-agent/src/http/*`（或网关命令处理模块）
- `crates/nova-agent/src/config/*`
- `crates/nova-agent/src/conversation/*_tests.rs`

## 详细设计

### 1. 接口定义

新增会话级命令（示意）：

- `session.system_prompt.reload`

请求参数：

- `sessionId: string`

响应：

- `sessionId`
- `versionBefore`
- `versionAfter`
- `updatedAt`
- `changed`

### 2. 执行流程

1. 根据 `sessionId` 获取 Session 运行时对象。
2. 在无锁或短锁区读取配置并执行提示词拼接。
3. 生成 `new_compiled_prompt` 和 `new_version`。
4. 进入写锁，比较当前版本并执行原子替换：
   - 更新 `system_prompt_compiled`
   - 更新 `system_prompt_version`
   - 更新 `system_prompt_updated_at`
   - 更新 `system_prompt_source_revision`
5. 返回结果并记录 info 日志。

### 3. 并发语义

1. 在请求开始构建模型输入时读取一次当前 `system_prompt_compiled` 快照。
2. 正在执行中的请求沿用其启动时快照，不受重载影响。
3. 重载完成后的新请求读取到新 prompt。

实现建议：

1. 使用 `tokio::sync::RwLock` 保护 Session 可变状态。
2. 禁止在持有写锁期间执行配置文件 IO。

### 4. 失败与回滚语义

1. 读取配置失败：直接返回错误，不改 Session 状态。
2. 拼接失败：直接返回错误，不改 Session 状态。
3. 写锁冲突或 Session 不存在：返回显式错误码，前端可提示重试。

### 5. 日志与观测

记录一条 `info!`：

- `session_id`
- `version_before`
- `version_after`
- `changed`
- `prompt_len`

错误路径用 `error!`，包含上下文，不输出 prompt 内容。

## 测试案例

1. 正常重载：配置变更后，后续请求使用新 prompt。
2. 无变化重载：返回成功且 `changed = false`。
3. 配置解析失败：接口返回错误，旧 prompt 保持。
4. 并发测试：请求 A 开始后触发重载，请求 A 用旧 prompt，请求 B 用新 prompt。
5. 高频重载测试：连续多次重载不出现死锁与状态错乱。

