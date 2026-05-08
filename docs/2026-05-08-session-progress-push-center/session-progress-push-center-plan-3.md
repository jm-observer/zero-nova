# Plan 3: 前端刷新恢复

## 前置依赖

- Plan 1
- Plan 2

## 本次目标

页面刷新后，前端主动拉取当前 Session 的可恢复进度快照，并重新建立 Session 订阅，让后续实时事件继续刷新页面。

可验证标准：

- 刷新后可恢复当前 Session 的消息、run 状态、权限状态
- 刷新后重新订阅当前 Session，后续实时事件可继续推进 UI
- 不依赖旧连接上的 token 增量继续播放

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/main.ts`
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/ui/agent-console-view.ts`
- 可能新增测试文件

## 详细设计

前端恢复流程：

1. WebSocket 连接成功
2. 读取当前 Session / workspace restore
3. 主动拉取：
   - `sessions.messages`
   - `session.runtime`
   - `session.runs`
   - `permission.pending`
4. 发送 Session 订阅请求到 Push Center
5. 后续增量更新继续走实时事件

恢复约束：

- 聊天窗口只恢复最终消息与当前运行态，不尝试恢复断开前未完成的 token 流文本
- 控制台中的 run / permission / diagnostics 以数据库快照为准，再叠加后续实时事件

## 测试案例

- 正常路径：任务运行中刷新页面，刷新后当前 Session 仍显示 `running`
- 正常路径：刷新后等待权限中的 request 仍显示在控制台
- 边界条件：刷新时 run 已经结束，恢复结果应显示最终状态而不是卡在 `running`
- 异常场景：恢复拉取成功但订阅失败，页面至少应显示快照，不得空白
