# Gateway / WebSocket 重构 Phase 1 文档

## 时间
- 创建时间：2026-04-22
- 最后更新：2026-04-22

## 项目现状
- `src/gateway/protocol.rs` 仍承担单一大枚举，混合已实现命令、未实现命令和事件消息。
- `src/gateway/router.rs` 通过兜底 `_` 分支统一返回 `NOT_IMPLEMENTED`，但协议层仍对前端暴露了大量暂不支持或长期 stub 的能力。
- `src/gateway/mod.rs`、`handlers/*`、`deskapp` 前端当前仍围绕这份大协议协作，因此第一阶段不能改动运行时边界，只能先收缩协议表面。
- 仓库中已有 [2026-04-22-remove-unimplemented-gateway-features.md](/D:/git/zero-nova/docs/2026-04-22-remove-unimplemented-gateway-features.md) 作为局部清理计划，但还没有把该动作放入整体阶段迁移路径。

## 本次目标
- 在不改变运行时装配方式的前提下，收缩 Gateway 对外协议到“当前真实闭环支持”的最小集合。
- 删除未落地命令、空分支和无测试支撑的 DTO，降低后续拆模块和拆库时的搬运范围。
- 为后续阶段建立稳定协议基线，保证前后端在 Phase 1 完成后仍能完整使用现有已交付功能。

## 详细设计

### 阶段范围
- 保留目录结构：`src/gateway` 暂不拆目录，不引入新 crate。
- 保留运行路径：`src/bin/nova_gateway.rs -> gateway::start_server -> gateway::server::run_server` 不变。
- 聚焦收缩的模块：
  - `src/gateway/protocol.rs`
  - `src/gateway/router.rs`
  - `src/gateway/handlers/*`
  - `deskapp` 中直接调用已删除协议的入口

### 目标协议基线
- 客户端命令仅保留：
  - `chat`
  - `chat.stop`
  - `sessions.list`
  - `sessions.messages`
  - `sessions.create`
  - `sessions.delete`
  - `sessions.copy`
  - `agents.list`
  - `agents.switch`
  - `config.get`
  - `config.update`
- 服务端事件 / 响应仅保留：
  - `welcome`
  - `error`
  - `chat.start`
  - `chat.progress`
  - `chat.complete`
  - `chat.stop.response`
  - `sessions.list.response`
  - `sessions.messages.response`
  - `sessions.create.response`
  - `sessions.delete.response`
  - `sessions.copy.response`
  - `agents.list.response`
  - `agents.switch.response`
  - `config.get.response`
  - `config.update.response`
  - `interaction.request`
  - `interaction.resolved`

### 明确删除的协议
- 从 `MessageEnvelope` 移除以下变体及其 DTO：
  - `agents.create`
  - `memory.stats`
  - `memory.stats.response`
  - `settings.get`
  - `browser.launch`
  - `browser.status`
  - `browser.status.response`
  - 以及已在前端同步下线、当前无后端完整闭环的其他占位命令
- 删除标准：
  - 当前无 handler
  - 当前无自动化测试
  - 未来一到两个迭代内没有明确 owner

### Router 收口策略
- `router.rs` 不再依赖“大而全”的协议暴露面做兜底吞吐。
- 对仍保留、但暂时不准备支持的命令，必须有显式枚举和显式错误路径；本阶段完成后原则上不应继续存在“枚举里声明但没有实现者”的情况。
- 若未来新增命令，必须同时满足三项：
  - 协议枚举已添加
  - 对应 handler 已实现
  - 至少有一条协议测试或路由测试

### DTO 收缩原则
- 不在 Phase 1 推动大规模类型化改造，但要先删除纯占位 DTO。
- 继续保留运行闭环所需的 `serde_json::Value` 字段，类型化工作延后到 Phase 3 之前逐步处理。
- 这样做的原因是本阶段目标是“减法”，避免把协议收缩和协议重塑混成一次大改。

## 实施步骤

### Step 1: 盘点协议真实使用面
- 核对 `router.rs` 已处理命令。
- 核对 `handlers/*` 中实际构造的响应与事件。
- 核对 `deskapp` 对 Gateway 的调用面，防止后端删协议但前端仍保留入口。

### Step 2: 删除协议中的未实现入口
- 从 `protocol.rs` 中移除未实现或仅占位的枚举变体。
- 同步删除失去引用的请求 / 响应 DTO。
- 保留 `Unknown`，用于承接非法或旧版消息。

### Step 3: 收敛 router 和 handlers
- 移除 router 中针对已删除协议的匹配分支。
- 保证所有保留命令都能映射到一个明确 handler。
- 保证未知命令统一转成标准错误响应，而不是被忽略。

### Step 4: 同步前端入口
- 删除 deskapp 中对下线协议的调用封装。
- 删除 UI 中会触发这些接口的入口，避免用户产生“按钮可点但后端不支持”的假象。

### Step 5: 补齐测试与兼容说明
- 为保留协议补上序列化 / 反序列化测试。
- 为未知消息增加错误路径测试。
- 对前端或调用方说明本阶段删除的协议清单。

## 阶段完成后的功能完整性要求
- 完成后程序必须继续完整支持以下用户路径：
  - 建立 WebSocket 连接并收到 `welcome`
  - 创建会话、列出会话、获取历史消息、复制会话、删除会话
  - 发起聊天、接收进度、正常完成聊天、停止聊天
  - 列出 Agent、切换 Agent
  - 获取和更新配置
  - 处理 `interaction.request` 与 `interaction.resolved`
- 完成后程序必须满足以下兼容性约束：
  - 已保留消息的 JSON 结构不做破坏性修改
  - 旧前端如果发送已下线命令，后端返回明确错误而不是静默失败
  - 新前端不再展示已删除能力入口

## 测试案例
- 协议测试：
  - `chat`、`sessions.*`、`agents.switch`、`config.*` 的序列化与反序列化通过
  - 非法或未知 `type` 能落到 `Unknown` 或统一错误路径
  - 已删除消息不再能作为合法协议通过编译或测试
- 路由测试：
  - 每个保留命令都能落到对应 handler
  - 非法消息返回 `NOT_IMPLEMENTED` 或 `INVALID_REQUEST`
- 端到端验证：
  - 使用现有前端完成一次建连、创建会话、发送聊天、列出会话、切换 Agent、更新配置

## 风险与待定项
- 风险：
  - 前端仍可能残留少量不再使用的文案或类型定义，需要在实际删入口时一并核对。
  - 如果旧版前端依赖已下线消息，Phase 1 会带来兼容性变化，需要通过明确错误提示降低排障成本。
- 待定项：
  - `auth` 是否近期要恢复。如果不恢复，建议在 Phase 1 一并标记为下线或仅保留统一错误语义，不再继续保留“看似即将支持”的入口。

