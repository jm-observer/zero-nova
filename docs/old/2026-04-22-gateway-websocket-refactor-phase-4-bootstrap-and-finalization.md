# Gateway / WebSocket 重构 Phase 4 文档

## 时间
- 创建时间：2026-04-22
- 最后更新：2026-04-22

## 项目现状
- 即使完成前 3 个阶段，`gateway::start_server` 当前仍承担工具注册、skills 加载、AgentRegistry 构造、AgentRuntime 构造、SQLite 初始化、Session 载入和 server 启动等全部装配职责。
- 这会让“应用启动”和“渠道启动”继续混在同一个入口里，渠道库虽然被抽出，但整体系统边界仍不够清晰。

## 本次目标
- 重写启动装配层，把依赖装配、应用服务构造和渠道启动分离。
- 让 `src/bin/nova_gateway.rs` 只负责读取配置、初始化日志和调用 bootstrap。
- 在 Phase 4 结束时，形成可长期维护的最终结构，并保持程序功能完整。

## 详细设计

### 目标启动链路
- 期望链路：
  - `src/bin/nova_gateway.rs`
  - `src/app/bootstrap.rs`
  - `GatewayApplication` / `ConversationService`
  - `channel_websocket::run(...)`

### `bootstrap` 职责
- 新增 `src/app/bootstrap.rs`
- 只负责：
  - 读取运行目录与配置
  - 构建 `ToolRegistry`
  - 加载 skills
  - 构建 `AgentRegistry`
  - 构建 `AgentRuntime`
  - 初始化会话存储与载入缓存
  - 组装应用服务
  - 组装渠道 handler
  - 启动 `channel-websocket`
- 不负责：
  - 具体协议路由
  - 单条消息处理逻辑
  - WebSocket 收发细节

### `GatewayApplication` 设计
- 在 Phase 2 的 service 基础上，进一步增加应用层门面：
  - `connect`
  - `handle`
  - `disconnect`
- 该门面接收应用命令，产出应用事件。
- 渠道层仅做：
  - 协议 DTO <-> 应用命令 / 事件转换

### 配置与持久化装配
- `config` 的读写路径仍由 bootstrap 注入，不放回渠道层。
- SQLite manager 和 repository 的实例化也在 bootstrap 完成，再注入 `SessionService`。
- 这样可以保证后续如果增加 HTTP、IPC 或测试桩渠道，复用同一应用层即可。

### 命名收尾
- 根据 Phase 4 完成后的真实职责，评估是否保留 `gateway` 命名：
  - 若 `src/gateway` 仅剩协议适配，可考虑更名为 `src/channel/gateway_protocol` 或 `src/presentation/gateway_ws`
  - 如果更名会放大变更面，可先保持目录名，但停止向其中放置业务逻辑

## 实施步骤

### Step 1: 提取 bootstrap
- 新增 `src/app/bootstrap.rs`
- 把 `gateway::start_server` 中的构造逻辑迁出
- 先保持输出对象和行为不变

### Step 2: 引入应用门面
- `router` 或 `GatewayChannelHandler` 不再直接持有多个底层对象，而是持有单一 `GatewayApplication`
- 将 connect / disconnect 的逻辑也显式化，避免未来渠道增加后重复拼装

### Step 3: 收尾旧入口
- 删除或废弃 `gateway::start_server`
- 让 `nova_gateway` 二进制入口直接走 bootstrap
- 清理旧的 re-export，避免新代码继续依赖历史入口

### Step 4: 文档和结构收口
- 更新总设计文档和目录文档
- 把阶段性文档中的“已完成”状态与最终结构对应起来
- 如果有必要，为新结构补充模块职责说明文档

## 阶段完成后的功能完整性要求
- Phase 4 完成后，程序对外功能必须与 Phase 3 一致，不允许因启动链路调整导致行为回退。
- 必须保证：
  - 二进制仍能正常启动
  - WebSocket 服务可连接
  - 所有保留协议功能继续工作
  - SQLite 会话可正确恢复
  - Agent 配置、skills、tools 的装配结果与重构前一致
- 结构完整性要求：
  - 启动装配和渠道启动解耦
  - 应用门面成为渠道调用唯一入口
  - 后续新增渠道不需要重新复制 Agent / Session 装配逻辑

## 测试案例
- 启动测试：
  - `nova_gateway` 可正常启动并监听端口
  - 启动时正确加载 tools、skills、agents、sessions
- 集成测试：
  - 从进程启动到客户端连接、创建会话、聊天完成的整条链路通过
  - 重启后 session 可恢复
- 回归测试：
  - 前 3 个阶段建立的全部测试继续通过

## 风险与待定项
- 风险：
  - `start_server` 中当前混有提示词文件读取与 workspace 路径逻辑，迁移时如果遗漏，容易导致运行时行为变化。
  - 启动链路重写后，日志初始化与配置注入顺序必须保持稳定，否则排障成本会上升。
- 待定项：
  - `gateway` 目录是否在 Phase 4 末尾立即更名。如果当前收益不明显，可以先稳定边界，后续再做命名清理。

