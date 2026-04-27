# 2026-04-27 crate consolidation design

## 时间
- 创建时间: 2026-04-27
- 最后更新: 2026-04-27

## 项目现状
- 当前 workspace 内部 crate 数量偏多，存在若干体量很小且职责高度相关的 crate:
  - `nova-server-stdio` 与 `nova-server-ws` 主要差异在入口和传输方式，核心网关能力重复装配。
  - `channel-core`、`channel-stdio`、`channel-websocket` 形成“抽象 + 两个实现”的组合，但目前仅在项目内使用，独立 crate 管理成本偏高。
- 同时存在边界清晰且应保持独立的 crate:
  - `nova-agent` 作为业务核心，不应绑定外部协议 DTO。
  - `nova-protocol` 作为网关 JSON 协议契约，供多接入形态复用。
  - `nova-gateway-core` 作为协议路由与桥接层，连接 protocol 与 agent。

## 整体目标
- 通过小步、可回滚的方式降低 crate 数量和依赖复杂度。
- 合并高耦合、低体量 crate，保留关键分层边界（agent/protocol/gateway）。
- 在每一步合并后保持行为一致，并通过统一修复流程验证。

## Plan 拆分
1. Plan 1: 统一 server 入口 crate（`nova-server-stdio` + `nova-server-ws`）
- 描述: 新建/改造 `nova-server` 作为统一 server crate，保留两个二进制入口。
- 依赖: 无
- 顺序: 第 1 步

2. Plan 2: 合并 channel 相关 crate 到 `nova-server` 模块内
- 描述: 将 `channel-core`、`channel-stdio`、`channel-websocket` 合并为 `nova-server::transport::{core, stdio, ws}`。
- 依赖: Plan 1
- 顺序: 第 2 步

3. Plan 3: 清理 workspace 成员与依赖映射
- 描述: 移除被合并 crate 的 workspace 成员与依赖声明，修复引用路径和 feature。
- 依赖: Plan 2
- 顺序: 第 3 步

4. Plan 4: 回归验证与发布前检查
- 描述: 运行全量 clippy/fmt/test，补充迁移说明和回滚策略。
- 依赖: Plan 3
- 顺序: 第 4 步

## 风险与待定项
- 风险 1: 传输层抽象从 crate 边界改为模块边界后，未来外部复用成本上升。
- 风险 2: WS 入口含父进程生命周期监控，迁移时易遗漏行为细节。
- 风险 3: 路径调整后可能出现 feature、`use` 路径、bin 名称不一致问题。
- 待定项 1: 是否保留与现有一致的二进制名（`nova_gateway_stdio`、`nova-server-ws`）以兼容脚本。
- 待定项 2: 是否需要保留过渡期 re-export 层（减少一次性改动冲击）。
