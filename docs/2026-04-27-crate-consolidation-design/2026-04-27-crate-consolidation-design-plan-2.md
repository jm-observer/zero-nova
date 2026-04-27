# Plan 2: 合并 channel crate 到 nova-server 模块

## Plan 编号与标题
- Plan 2: 合并 `channel-core`、`channel-stdio`、`channel-websocket`

## 前置依赖
- Plan 1

## 本次目标
- 将 channel 抽象与传输实现收敛到 `nova-server` 内部模块。
- 保留 `ChannelHandler`/`ResponseSink` 语义不变，减少上层改动。
- 保持 stdio/ws 行为与并发模型一致。

## 涉及文件
- 新增: `crates/nova-server/src/transport/core.rs`
- 新增: `crates/nova-server/src/transport/stdio.rs`
- 新增: `crates/nova-server/src/transport/ws.rs`
- 新增: `crates/nova-server/src/transport/mod.rs`
- 修改: `crates/nova-server/src/lib.rs`
- 修改: `crates/nova-gateway-core/src/lib.rs`（`ChannelHandler` 引用路径）
- 修改: `crates/nova-server-ws`/`nova-server-stdio` 迁移代码（若仍保留过渡）

## 详细设计
- 模块边界:
  - `transport::core` 持有 trait 与 sink 抽象。
  - `transport::stdio`、`transport::ws` 仅依赖 `transport::core`。
- 依赖收敛:
  - 将 `tokio-tungstenite` 等仅在 ws 使用的依赖放在 `nova-server` 中，避免分散。
- 行为一致性:
  - stdio: 仍使用 NDJSON 行级协议。
  - ws: 保留连接事件、消息反序列化、ping/pong、断连回调、写任务。
- 迁移方式:
  - 优先复制原逻辑后替换路径。
  - 编译通过后再删除旧 crate，避免一次性大改导致排障困难。

## 测试案例
- 正常路径:
  - 单连接 ws 收发请求并返回响应。
  - stdio connect/message/disconnect 生命周期正确触发。
- 边界条件:
  - ws 高频消息下 outbound channel 容量达到上限时行为可预期。
  - ws 收到 ping 时正确回 pong。
- 异常场景:
  - ws 收到非法 JSON 记录 warn，不崩溃。
  - 业务层 handler 返回错误时日志可诊断且连接流程可继续/结束符合原行为。
