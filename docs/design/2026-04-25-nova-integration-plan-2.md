# zero-nova 集成 Plan 2 - 协议层增强

## 文档说明

- 设计文档: docs/design/2026-04-25-nova-integration-design.md
- 影响项目: zero-nova (主要), zero (次要)
- Phase: Phase 3 (增强阶段)

---

## 一、改动范围

### 1.1 影响文件清单

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| crates/nova-app/src/application.rs | 修改 | finish_turn() 方法 |
| crates/nova-conversation/src/ | 修改 | export_session() 方法 |
| crates/nova-protocol/src/envelope.rs | 修改 | WeChat 扩展字段 |

### 1.2 不改动文件

- crates/nova-core/ - Agent 核心正常运行
- crates/nova-gateway-core/ - 网关核心
- crates/channel-*/ - 通道系统

---

## 二、实施步骤

### Step 1: 添加 finish_turn 方法

**文件**: crates/nova-app/src/application.rs

```
pub async fn finish_turn(&self, session_id: &str, turn_id: TurnId) -> Result<FinishTurn> {
    // 聚合 Agent 运行结果，清除 turn 状态
}
```
