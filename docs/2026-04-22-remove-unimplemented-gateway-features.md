# 删除未实现 Gateway 接口计划

## 时间
- 创建时间：2026-04-22
- 最后更新：2026-04-22

## 项目现状
- 后端 `src/gateway/router.rs` 仅实际处理了聊天、会话基础操作、Agent 列表/切换、配置获取/更新。
- `router.rs` 第 77-85 行中有一批消息类型被直接忽略，不返回成功响应也不返回错误响应。
- `deskapp` 前端仍然暴露了成果物面板、调度器视图、标题栏浏览器按钮，以及设置页中的 Router / 微信 / 进化入口。
- 其中前端实际调用了未实现接口的地方主要有：
  - `sessions.artifacts`
  - `scheduler.list`
  - `scheduler.runs`
  - `browser.launch`
- 另外设置页模板仍展示 Router / 微信 / OpenFlux 相关占位入口，但当前并没有完成的业务逻辑支撑。

## 本次目标
- 删除后端明确未实现且当前不准备交付的 Gateway 协议入口。
- 删除前端对应的占位服务、视图注册与可见入口，避免用户触发无效能力。
- 保持本次变更聚焦，不重构已实现的聊天、会话、配置能力。

## 详细设计
- 后端删除范围：
  - 从 `gateway::protocol` 中移除未实现且本次前端同步下线的消息枚举与辅助结构。
  - 从 `gateway::router` 中移除对应匹配分支。
  - 删除仅为占位返回空数据的 `scheduler` handler 模块。
- 前端删除范围：
  - 移除 `ArtifactsView`、`SchedulerView` 的注册与源码文件。
  - 从 `GatewayClient` 中删除对应未实现 API 封装。
  - 删除标题栏中的浏览器启动按钮与成果物面板按钮。
  - 删除侧边栏中的调度器入口，以及页面中的调度器 / 成果物面板 DOM。
  - 删除设置页中 Router 工作模式卡片与 Router / 微信 / 进化页签，避免继续对用户暴露占位能力。
- 保留范围：
  - `config.get` / `config.update`
  - 聊天、会话、Agent 基础能力
  - 语音覆盖层本地 UI（与本次 Gateway 未实现接口删除不直接耦合）

## 测试案例
- 后端编译通过，`cargo clippy --workspace -- -D warnings` 无告警。
- `cargo fmt --check --all` 通过。
- `cargo test --workspace` 通过。
- 手工检查前端代码：
  - `main.ts` 不再注册成果物/调度器视图。
  - `titlebar.ts` 不再调用 `browser.launch`。
  - `gateway-client.ts` 不再暴露已删除的占位 API。
  - `index.html` 中不再出现调度器入口、成果物面板、浏览器按钮。

## 风险与待定项
- `deskapp` 中可能仍保留少量未使用的翻译文案和样式，本次不做大范围清扫，避免将样式整理混入功能删除。
- 如果后续要重新引入 Router / 微信 / 调度器，需要先补齐后端协议与 handler，再恢复前端入口，而不是只恢复模板按钮。
