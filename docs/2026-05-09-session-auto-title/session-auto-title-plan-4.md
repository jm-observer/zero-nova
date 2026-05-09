# Plan 4: 测试补充与回归验证

## 前置依赖
- Plan 1（标题生成组件建模与配置）
- Plan 2（后端触发策略与生成逻辑）
- Plan 3（网关协议与前端同步链路）

## 本次目标
- 为自动标题需求补足后端单测、集成测试与前端状态测试
- 确保"标题生成失败不影响聊天"和"延迟生成后 UI 可见"两个核心承诺可回归验证
- 补充 Plan 1-3 中遗漏的关键测试用例

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/session_title_service.rs`（Plan 1 新建）
- `crates/nova-agent/tests/integration/*`
- `deskapp/src/__tests__/chat-service.test.ts`
- `deskapp/src/__tests__/gateway-client-contract.test.ts`
- `deskapp/e2e/tests/sessions.e2e.spec.ts`

## 详细设计

### 1. 后端单元测试

#### 1.1 标题状态机测试
- 覆盖标题状态机：
  - 默认标题 session 初始为 `idle + source=default`
  - 第 1 条消息不触发
  - 第 2 条消息触发 `pending`
  - 成功后转 `succeeded`，不会再次触发
  - 失败后记录 `attempt_count` 和 `last_error`

#### 1.2 manual 修改保护测试
- **新增用例**：标题已被 manual 修改后，后续消息不得覆盖
- 验证 `source == Manual` 时 `should_generate()` 返回 `false`
- 验证手动修改后即使状态为 `failed` 也不自动重试

#### 1.3 标题规范化测试
- **新增用例**：生成成功但返回超长/非法字符时的规范化行为
- 验证标题长度超出 `max_title_length` 时被截断
- 验证标题包含换行符、引号等非法字符时被清理
- 验证空标题或纯空白标题被替换为默认值

#### 1.4 tokio::spawn 任务失败回收测试
- **新增用例**：tokio::spawn 任务失败（panic/cancel）时状态回收
- 模拟生成任务 panic，验证状态回退到 `pending` 而非 `generating`
- 验证任务被 cancel 后不会阻塞后续触发

### 2. 后端集成测试

#### 2.1 构造 fake title generator
- 成功模式：断言第二或第三条用户消息后标题被写回 repository
- 失败模式：断言 `start_turn` 仍返回成功，session 标题维持默认值

#### 2.2 验证并发
- 快速连续两次消息只会产生一次并发中的标题任务
- 验证 CAS 更新为 `generating` 的原子性

### 3. 前端测试

#### 3.1 ChatService
- 首条消息建 session 时不再使用用户输入截断标题
- 验证创建会话时调用后端 `createSession` 不传 title

#### 3.2 GatewayClient
- 能解析并分发新的 session summary 更新事件
- **新增用例**：网关事件乱序/重复到达时前端幂等更新
- 验证收到相同 sessionId + title 的重复事件时不触发多余重渲染
- 验证事件乱序到达时最终状态正确

#### 3.3 state / view
- session 标题更新后 sidebar 与 chat header 均反映新值
- 验证标题事件到达时用户已切到其他 session，只更新 state，不误改当前消息区

### 4. E2E 回归
- 新会话进入时显示默认标题
- 连续发送 2 到 3 条消息后，标题自动变为 AI 结果
- 标题生成失败场景下，聊天功能继续可用，默认标题保留

## 测试案例

### 正常路径
- 会话从 `New Chat` 自动切换为语义标题
- 标题生成成功后立即通过事件推送到前端
- 前端收到事件后更新 sidebar 和 chat header

### 边界条件
- 短消息如"你好""继续"不应过早触发标题
- 成功生成后继续聊天，标题保持稳定
- 标题已被 manual 修改后，后续消息不得覆盖
- 生成成功但返回超长/非法字符时被规范化
- tokio::spawn 任务失败（panic/cancel）时状态正确回收
- 网关事件乱序/重复到达时前端幂等更新

### 异常路径
- 生成器抛错不影响消息展示、turn 完成事件和后续对话
- 网关事件晚到或重复到达时，UI 最终状态仍正确
- 标题生成连续失败达到 max_attempts 后停止尝试
