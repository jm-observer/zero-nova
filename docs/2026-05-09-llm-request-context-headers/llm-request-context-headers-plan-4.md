# Plan 4: 测试、验证与发布回归

## 前置依赖
- Plan 1
- Plan 2
- Plan 3

## 本次目标
- 补齐单元测试/集成测试覆盖。
- 明确验证步骤与上线回滚策略。
- 确保修复流程（clippy/fmt/test）全量通过。

## 涉及文件
- `crates/nova-agent/tests/integration/*`（新增或扩展）
- `crates/nova-agent/src/provider/openai_compat/mod.rs`（测试模块）
- `docs/2026-05-09-llm-request-context-headers/llm-request-context-headers.md`（更新 Plan 状态）

## 详细设计

### 1. 测试分层

单元测试：
- Header 构建函数：空值过滤、trim、生效条件。
- 开关逻辑：`enabled` true/false 分支。

集成测试：
- 模拟 OpenAI 兼容服务，断言收到 Header。
- 验证流式消息处理不回归。

回归测试：
- 现有会话聊天路径、tool call 路径、max_tokens continue 路径。

### 2. 验证步骤
1. 启动本地 mock LLM endpoint，记录请求 Header。
2. 发起会话请求，确认 `x-session-id` / `x-agent-id` 出现且值正确。
3. 关闭配置开关后重复请求，确认 Header 消失。
4. 观察日志与前端交互，无额外报错或延迟异常。

### 3. 发布与回滚
- 发布前执行修复流程：
  1. `cargo clippy --workspace -- -D warnings`
  2. `cargo fmt --all --check`
  3. `cargo test --workspace`
- 若线上发现网关不兼容：将 `outbound_context_headers.enabled=false` 作为一键回滚。

## 测试案例
1. Header 透传开启 + 两个字段齐全。
2. Header 透传开启 + 单字段缺失。
3. Header 透传关闭 + 字段齐全。
4. 并发 10 个 session 请求，header 与 session 一一对应。
5. 失败场景：上游返回 4xx 时，错误链路可观测且不吞错。
