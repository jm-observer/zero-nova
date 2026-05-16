# 移除过度设计 — 总览文档

- 创建时间: 2026-05-16
- 状态: 待实施

## 项目现状

`nova-agent` crate 存在以下过度设计问题：
1. `LlmClient` 作为泛型参数感染整条调用链（AgentRuntime<C> → ConversationService<C> → AgentApplicationImpl<C>）
2. `AgentApplication` trait 有 100+ 方法但只有 1 个实现，存在的唯一目的是擦除泛型
3. `TitleGenerator` trait 只有 1 个真实实现（6 行逻辑），是投机性抽象
4. `storage/` 子模块只有 1 个 17 行函数，是空壳层

## 整体目标

- 将 `LlmClient` 从泛型参数改为 `Box<dyn LlmClient>`，消除泛型感染
- 移除 `AgentApplication` trait，gateway 直接使用具体类型
- 移除 `TitleGenerator` trait，内联标题生成逻辑
- 合并 `storage/` 到 repository 层

## Plan 拆分

| Plan | 标题 | 依赖 | 说明 |
|------|------|------|------|
| Plan 1 | 消除 LlmClient 泛型感染 | 无 | 核心改动，影响面最大 |
| Plan 2 | 移除 AgentApplication trait | Plan 1 | Plan 1 完成后 AgentApplicationImpl 不再是泛型，trait 可移除 |
| Plan 3 | 移除 TitleGenerator trait | 无 | 独立改动 |
| Plan 4 | 合并 storage 到 repository | 无 | 独立改动 |

## 风险

- Plan 1 影响面广（nova-agent, nova-agent-loader, nova-cli, 集成测试），需要逐步验证
- Plan 2 影响 nova-gateway-core 的所有 handler，但改动是机械性的（`&dyn AgentApplication` → `&AgentApplicationImpl`）
