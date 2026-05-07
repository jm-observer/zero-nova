# Plan 1: Prompt 配置与重载模型定义

## 前置依赖
无

## 本次目标

定义“配置驱动的 system prompt 重载”所需的后端模型与协议字段，统一“当前生效值”“来源配置值”“版本指纹”的表达，避免后续实现阶段语义分叉。

## 涉及文件

- `crates/nova-agent/src/conversation/control.rs`（或对应 Session 状态定义文件）
- `crates/nova-agent/src/config/*`（提示词配置读取与结构体）
- `crates/nova-protocol/src/observability.rs`（Session 视图扩展）
- `deskapp/src/core/types.ts`（前端类型对齐）

## 详细设计

### 1. 统一数据来源

1. 配置文件中的提示词片段为唯一 source of truth。
2. Session 内仅缓存“已拼接的最终 prompt + 元信息”，不保存独立编辑副本。
3. 禁止通过 API 直接写入最终 prompt 文本。

### 2. Session 内 Prompt 状态结构

建议在 Session 运行时状态增加：

- `system_prompt_compiled: String`：当前生效的拼接结果。
- `system_prompt_version: String`：版本指纹（建议 `sha256(compiled_prompt)` 的十六进制字符串）。
- `system_prompt_updated_at: i64`：毫秒时间戳。
- `system_prompt_source_revision: String`：配置源版本（可用配置文件 mtime + size，或配置内容哈希）。

### 3. 重载结果模型

定义重载返回结构（示意）：

- `session_id: String`
- `version_before: String`
- `version_after: String`
- `updated_at: i64`
- `changed: bool`（拼接结果是否变化）

约束：

1. 若配置读取成功但拼接结果无变化，`changed = false`，仍返回成功。
2. 若读取或拼接失败，不更新内存态字段，返回错误。

### 4. 版本策略

1. 版本以最终拼接结果哈希为准，确保“内容相同则版本相同”。
2. 配置源版本仅用于诊断，不用于业务判断“是否更新成功”。
3. 日志只记录版本与长度，不打印 prompt 明文。

### 5. 协议扩展

在 session 观测或详情协议中新增只读字段：

- `systemPromptVersion`
- `systemPromptUpdatedAt`

可选新增：

- `systemPromptSourceRevision`

不对外返回完整 prompt 内容（默认），避免泄露。

## 测试案例

1. 同一配置重复重载，`version_after == version_before` 且 `changed = false`。
2. 修改配置片段后重载，版本变化且 `changed = true`。
3. 配置读取失败时返回错误，Session 旧版本不变。
4. 拼接器失败时返回错误，Session 旧版本不变。
5. 序列化/反序列化协议字段与前端类型命名保持 camelCase 一致。

