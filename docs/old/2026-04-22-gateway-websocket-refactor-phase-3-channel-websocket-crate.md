# Gateway / WebSocket 重构 Phase 3 文档

## 时间
- 创建时间：2026-04-22
- 最后更新：2026-04-22

## 项目现状
- `src/gateway/server.rs` 仍是主 crate 内部模块，直接配合 `router.rs` 与 `GatewayMessage` 工作。
- 即使经过 Phase 2 业务层已被抽离，WebSocket server 仍然和当前 crate 的协议类型、handler 形态紧耦合，尚未具备“渠道库”复用价值。
- `Cargo.toml` 当前 workspace 只有根 crate 和 `deskapp/src-tauri`，还没有单独的渠道 crate。

## 本次目标
- 提取独立的 `channel-websocket` workspace 成员，只负责 WebSocket 渠道能力。
- 让主 crate 通过 trait / handler 接口接入该库，而不是继续在 `src/gateway/server.rs` 中绑定具体应用状态。
- 保证提取后协议语义不变、行为不变，程序依然完整提供现有 Gateway 功能。

## 详细设计

### crate 设计
- 新增 workspace 成员：`crates/channel-websocket`
- 根 `Cargo.toml` 需要调整 workspace members，并把共享依赖迁移到 `[workspace.dependencies]`
- 遵循仓库约束：
  - workspace 成员依赖统一用 `{ workspace = true }`
  - 不引入不必要的新依赖；优先复用已有 `tokio`、`tokio-tungstenite`、`serde`、`anyhow`

### `channel-websocket` 职责范围
- 只包含：
  - WebSocket 监听与 accept
  - 连接收包 / 发包循环
  - 文本帧 JSON 编解码
  - Ping / Pong / Close
  - 连接级错误边界
  - 回调式 handler 接口
- 不包含：
  - `AgentRuntime`
  - `SessionStore`
  - `ConversationService`
  - SQLite
  - Config 持久化

### 协议放置策略
- Phase 3 建议先维持协议仍在主 crate，避免同阶段同时做“拆库 + 协议完全独立”。
- `channel-websocket` 通过泛型请求 / 响应类型接入：
  - 请求与响应只要求可反序列化 / 序列化
  - 库本身不关心 `GatewayMessage` 代表什么业务
- 可选接口形式：

```rust
#[async_trait::async_trait]
pub trait ChannelHandler: Send + Sync + 'static {
    type Request;
    type Response;

    async fn on_connect(&self, peer: std::net::SocketAddr) -> anyhow::Result<Vec<Self::Response>>;
    async fn on_message(
        &self,
        peer: std::net::SocketAddr,
        message: Self::Request,
        sink: ResponseSink<Self::Response>,
    ) -> anyhow::Result<()>;
    async fn on_disconnect(&self, peer: std::net::SocketAddr) -> anyhow::Result<()>;
}
```

### 主 crate 接入设计
- 在主 crate 中实现 `GatewayChannelHandler` 或 `WebSocketGatewayHandler`：
  - 入站：`GatewayMessage` -> 应用命令
  - 出站：应用事件 -> `GatewayMessage`
- 现有 `router.rs` 可以保留一层，但建议逐步退化为 `GatewayApplication` 调用适配器。

### 背压与连接生命周期
- `channel-websocket` 要显式管理：
  - 每连接 outbound channel
  - 发送循环退出
  - 接收循环退出
  - 上层 sink 关闭后的资源释放
- 这部分必须在库中收口，避免主 crate 再次复制一套连接管理逻辑。

## 实施步骤

### Step 1: 建立 crate 骨架
- 新建 `crates/channel-websocket/Cargo.toml`
- 新建 `src/lib.rs`
- 添加 `server.rs`、`connection.rs`、`codec.rs`
- 调整根 workspace 成员和共享依赖

### Step 2: 迁移 server 公共能力
- 从现有 `src/gateway/server.rs` 中抽取通用连接管理逻辑。
- 去掉对 `AppState` 和 `GatewayMessage` 的直接依赖。
- 用 trait 回调把业务处理交还给主 crate。

### Step 3: 在主 crate 实现 handler
- 新增 `GatewayChannelHandler`
- 连接时发送 `welcome`
- 收到请求时调用 Phase 2 提炼出的应用服务
- 断连时执行必要的清理

### Step 4: 替换启动路径
- `gateway::start_server` 或后续 bootstrap 改为调用 `channel_websocket::run(...)`
- 保持监听地址、配置与日志行为不变

## 阶段完成后的功能完整性要求
- Phase 3 完成后，用户感知到的 Gateway 能力必须和 Phase 2 完全一致。
- WebSocket 对外行为必须保持：
  - 仍监听同一地址与端口
  - 仍使用相同 JSON 消息协议
  - 仍支持并发连接
  - 非法 JSON 不会导致整个 server 崩溃
  - 客户端断开后不会遗留失控发送任务
- 代码边界必须满足：
  - `src/gateway/server.rs` 被删除或仅保留极薄兼容层
  - WebSocket 通用能力位于独立 crate
  - 主 crate 不再自己实现底层 WebSocket accept / read / write 循环

## 测试案例
- crate 级测试：
  - 文本消息可正确 decode / encode
  - 非法 JSON 返回错误，不导致进程退出
  - 客户端主动断开时连接任务正确退出
  - sink 关闭后发送循环终止
- 集成测试：
  - 使用主 crate 的 `GatewayChannelHandler` 建立连接后仍能完成 `welcome -> sessions.create -> chat -> chat.complete`
  - 多连接并发不会互相污染 session 流程
- 回归测试：
  - Phase 1 和 Phase 2 的协议与 service 测试全部继续通过

## 风险与待定项
- 风险：
  - 若在同一阶段同时移动协议模块到 crate 内，会明显放大改动面，建议避免。
  - workspace 依赖统一改造可能波及 `deskapp/src-tauri`，需要小步提交并及时验证。
- 待定项：
  - 后续是否要在 `channel-websocket` 上抽象出多协议支持。如果没有明确需求，Phase 3 只做单协议泛型化即可，不做过度抽象。

