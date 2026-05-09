# Plan 3: 标题生成器抽象与失败状态机收敛

## 前置依赖
- Plan 1

## 本次目标
- 将当前“拼接文本”逻辑替换为可注入的标题生成器接口。
- 明确超时、可重试错误、不可重试错误处理语义。
- 消除 `Pending` 卡死风险，保证每次尝试都有终态。

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/control.rs`
- `crates/nova-agent/src/provider/*` 或新增 `crates/nova-agent/src/conversation/title_generator.rs`
- `crates/nova-agent/src/config/*`（若将超时和重试策略配置化）

## 详细设计
- 生成器接口：
  - 定义 trait：`TitleGenerator`，方法 `async fn generate_title(&self, user_texts: &[String]) -> Result<String, TitleGenerationError>`。
  - 在 `SessionService` 注入 `Arc<dyn TitleGenerator + Send + Sync>`，默认实现可先保留本地轻量规则，后续替换 LLM 实现。
- 错误建模：
  - `TitleGenerationError::Retryable(anyhow::Error)`（网络/超时/服务不可用）
  - `TitleGenerationError::NonRetryable(anyhow::Error)`（空响应/格式错误/策略拒绝）
- 状态机修复：
  - `set_pending` 后，无论成功/失败/panic 都必须进入终态。
  - 推荐使用守卫模式：若函数异常退出且状态仍为 `Pending`，统一回填为 `Failed`。
  - 重试条件由 `status == Failed && attempt_count < TITLE_MAX_ATTEMPTS` 控制。
- 策略常量：
  - 保留并集中管理：最小消息数、最小字符数、最大尝试数、超时时间。
  - 禁止散落 magic number。

## 测试案例
- 正常路径：
  - mock 生成器返回标题，状态为 `Succeeded`，`source=Ai`，标题落库。
- 边界条件：
  - 生成结果经 normalize 后为空，状态变 `Failed`，可触发下一次重试。
- 异常路径：
  - mock 生成器返回 Retryable 错误，状态 `Failed` 且尝试次数+1。
  - mock 生成器超时，状态 `Failed`，不会阻塞 `append_message` 返回。
  - 异步任务 panic 后不留 `Pending` 悬挂状态。
