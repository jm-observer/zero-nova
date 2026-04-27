# Plan 1: 统一 server 入口 crate

## Plan 编号与标题
- Plan 1: 统一 server 入口 crate（`nova-server-stdio` + `nova-server-ws`）

## 前置依赖
- 无

## 本次目标
- 引入统一 crate `nova-server`，承载两个入口二进制。
- 保持既有启动参数、日志初始化、运行行为不变。
- 不在本 Plan 处理 channel crate 合并，仅做 server 入口收口。

## 涉及文件
- 新增: `crates/nova-server/Cargo.toml`
- 新增: `crates/nova-server/src/lib.rs`
- 新增: `crates/nova-server/src/bin/nova_gateway_stdio.rs`
- 新增: `crates/nova-server/src/bin/nova_gateway_ws.rs`
- 修改: `Cargo.toml`（workspace members、workspace.dependencies）
- 修改: 可能涉及启动脚本/README（如存在）

## 详细设计
- 结构设计:
  - `src/lib.rs` 暴露 `run_stdio`、`run_server` 两个统一入口函数。
  - `src/bin/nova_gateway_stdio.rs`、`src/bin/nova_gateway_ws.rs` 分别迁移原两个 crate 的 main 逻辑。
- 兼容策略:
  - 二进制名默认保持不变，避免影响已有调用方。
  - CLI 参数保持原样，避免 deskapp/脚本联动修改。
- 代码迁移原则:
  - 仅搬迁文件和路径，不调整业务逻辑。
  - 保持日志字段、错误路径、退出行为一致。

## 测试案例
- 正常路径:
  - `nova_gateway_stdio --workspace <path>` 能正常启动并响应请求。
  - `nova_gateway_ws --host 127.0.0.1 --port 9090` 能正常监听。
- 边界条件:
  - 缺失配置文件时返回与原行为一致的错误。
  - CLI 覆盖参数（`--model`、`--base_url`）生效。
- 异常场景:
  - WS 端口占用时返回可诊断错误。
  - stdio 输入非法 JSON 时服务不中断并打印错误日志。
