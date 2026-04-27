# Plan 3: 前端消费同一 Schema（类型生成 + 运行时校验）

## 前置依赖
- Plan 2

## 本次目标
- 前端类型从共享 schema 自动生成，替代手写协议类型。
- 前端网关通信收发两侧加入 runtime 校验，提前阻断不兼容数据。
- 保持前端业务代码改动最小，集中改造在网关数据边界层。

## 涉及文件
- `deskapp/src/core/types.ts`
- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/`（新增 generated 类型目录）
- `schemas/`（消费 Plan 2 产物）
- `deskapp/package.json`（新增脚本命令，依赖待确认）

## 详细设计
### 1. 类型来源切换
- 新增 `deskapp/src/core/generated/` 存放由 schema 生成的 TypeScript 类型。
- `types.ts` 对协议相关类型改为 re-export，不再手写协议字段。
- UI 专属 view model（纯前端状态）继续保留在 `types.ts`，与协议类型解耦。

### 2. 收发校验边界
- 发送请求前：按对应 request schema 校验；失败直接在前端报错并附带字段路径。
- 接收响应/事件后：按 envelope + payload schema 校验；失败记录错误日志并进入降级处理。
- 校验逻辑集中封装在 gateway client，不向页面层泄漏 schema 细节。

### 3. 降级策略
- 单条消息校验失败时不导致客户端崩溃；进入“协议错误事件”分支，保留可诊断信息。
- 连续失败超过阈值触发连接重置或提示用户升级版本。

### 4. 兼容迁移
- 先覆盖高频消息（chat/session），再覆盖 observability 细分事件。
- 迁移期允许并存：旧手写类型 + 新生成类型，但最终以生成类型为准。

## 测试案例
- 正常路径：合法 chat 响应可通过校验并驱动 UI 更新。
- 边界条件：可选字段缺失时前端保持兼容渲染。
- 异常场景：后端返回未知字段类型（如 number -> string）时前端校验失败并记录协议错误。
