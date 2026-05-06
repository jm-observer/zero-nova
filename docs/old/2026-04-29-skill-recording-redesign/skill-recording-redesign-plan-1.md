# Plan 1: 会话级模型与兼容策略

## 前置依赖
无

## 本次目标
在 `ControlState` 引入会话级 `skill_bindings`，并保证对历史会话 JSON 的反序列化兼容。

## 涉及文件
- `crates/nova-agent/src/conversation/control.rs`

## 详细设计
1. **模型新增**
- 在 `ControlState` 增加字段：`skill_bindings: Vec<serde_json::Value>`。
- 字段使用 `#[serde(default)]`，确保历史 JSON 缺少该字段时自动回落为空数组。

2. **初始化与约束**
- 在 `ControlState::new` 初始化 `skill_bindings` 为空。
- `skill_bindings` 存储 `SkillBindingSnapshot` 的序列化值，仅允许最小字段：`skill_id/name/status/description`。

3. **语义边界**
- `last_turn_snapshot.skills` 继续表达“最近回合技能快照”。
- `skill_bindings` 表达“会话级持久化技能绑定集合”。
- 禁止在后续实现中以 `last_turn.skills` 反推会话级完整状态。

## 测试案例
- **历史兼容测试**：反序列化不包含 `skill_bindings` 的旧 JSON，结果应成功且为空数组。
- **序列化测试**：包含 `skill_bindings` 的 `ControlState` 能正确序列化并回读。
- **边界测试**：`skill_bindings` 为空、单元素、多元素时结构稳定。
