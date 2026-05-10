# Plan 3: conversation/* 与 app/* 拆分设计

## 前置依赖
- Plan 1

## 本次目标
- 将会话域（`conversation`）和应用服务域（`app`）按“接口层/编排层/存储层”拆分。
- 降低 `service.rs`、`repository.rs`、`sqlite_manager.rs` 与 `app` 服务之间的耦合。
- 建立统一的事务边界与数据转换边界，便于后续测试隔离。

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/repository.rs`
- `crates/nova-agent/src/conversation/sqlite_manager.rs`
- `crates/nova-agent/src/conversation/mod.rs`
- `crates/nova-agent/src/conversation/service/commands.rs`（新增）
- `crates/nova-agent/src/conversation/service/queries.rs`（新增）
- `crates/nova-agent/src/conversation/service/events.rs`（新增）
- `crates/nova-agent/src/conversation/repository/session_repo.rs`（新增）
- `crates/nova-agent/src/conversation/repository/message_repo.rs`（新增）
- `crates/nova-agent/src/conversation/storage/sqlite_tx.rs`（新增）
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/mod.rs`

## 详细设计
1. 会话域分层
- `conversation/service/*`：应用编排层，拆分为命令（写）、查询（读）、事件推送。
- `conversation/repository/*`：持久层接口实现，按实体拆分（session/message）。
- `conversation/storage/*`：底层 sqlite 管理与事务 helper，避免业务逻辑混入 SQL 细节。

2. 应用层分层
- `app/application.rs` 聚焦装配（bootstrap/wire），不承载细粒度业务。
- `agent_workspace_service.rs` 聚焦项目目录、工作区状态与路径规则。
- `conversation_service.rs` 仅作为 facade，调用 `conversation/service/*`，不重复仓储逻辑。

3. 关键边界
- 读写分离：查询路径不持有可变状态锁，写入路径显式事务。
- DTO 与领域模型转换集中到单点（如 `model_mapper`）避免重复。
- 错误语义统一：仓储层返回带上下文错误，应用层不吞错。

## 测试案例
- 正常路径：创建会话、追加消息、查询历史、更新项目目录流程回归。
- 边界条件：空会话、分页边界、并发写入冲突。
- 异常场景：数据库连接异常、事务提交失败、路径越界访问。
