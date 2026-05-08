# 开发项目提示词 - 代码实现 Review

- **时间**: 2026-05-08
- **设计文档**: developer-project-prompts.md
- **状态**: 实现完成，Review 通过

---

## 一、配置模型 (Plan 1) ✅

### 1.1 AppConfig.developer_prompt_files
- **文件**: `crates/nova-agent/src/config.rs:34-37`
- **状态**: ✅ 顶层字段，默认空列表，有中文注释

### 1.2 RawAppConfig.developer_prompt_files
- **文件**: `crates/nova-agent/src/config.rs:693`
- **状态**: ✅ 在 migrate 中正确传递 (config.rs:959)

### 1.3 AgentSpec.enable_project_developer_prompt
- **文件**: `crates/nova-agent/src/config.rs:266-268`
- **状态**: ✅ 默认值为 `false`

### 1.4 PromptConfig.developer_project_prompt_content
- **文件**: `crates/nova-agent/src/prompt.rs:64`
- **状态**: ✅ 默认 `None`

### 1.5 校验：空字符串拒绝
- **文件**: `crates/nova-agent/src/config.rs:652-657`
- **状态**: ✅ 在 validate() 中检查，使用 `trim()` 处理空白字符串

### 1.6 测试覆盖
- ✅ `developer_prompt_files_empty_string_is_rejected`
- ✅ `developer_prompt_files_defaults_to_empty_list`
- ✅ `agent_enable_project_developer_prompt_defaults_to_false`
- ✅ `agent_enable_project_developer_prompt_can_be_set_to_true`

---

## 二、提示词加载与拼装 (Plan 2) ✅

### 2.1 SectionName::DeveloperProjectPrompt
- **文件**: `crates/nova-agent/src/prompt.rs:504-516`
- **状态**: ✅ 枚举成员存在，标题为 "Developer Project Instructions"

### 2.2 加载函数 load_developer_project_prompt
- **文件**: `crates/nova-agent/src/prompt.rs:471-501`
- **状态**: ✅ 完整实现

| 功能 | 状态 |
|------|------|
| 仅扫描项目根目录 | ✅ |
| 按配置列表顺序读取 | ✅ |
| 跳过不存在文件 | ✅ |
| 跳过空文件 | ✅ |
| 单文件失败不影响整体 | ✅ |
| 全部失败返回 None | ✅ |
| 格式：`### Source: <filename>\n<content>` | ✅ |
| 分隔符：`\n\n---\n\n` | ✅ |
| 整文件合并，无长度限制 | ✅ |
| 无递归，无向上查找 | ✅ |

### 2.3 异步版本 load_developer_project_prompt_async
- **文件**: `crates/nova-agent/src/prompt.rs:438-468`
- **状态**: ✅ 与同步版本逻辑一致

### 2.4 拼装顺序
- **文件**: `crates/nova-agent/src/prompt.rs:781-839`
- **状态**: ✅ Base → BehaviorGuards → Skill → DeveloperProjectPrompt → ProjectContext → Environment → Workflow

### 2.5 测试覆盖
- ✅ `load_developer_project_prompt_empty_project_dir`
- ✅ `load_developer_project_prompt_single_file`
- ✅ `load_developer_project_prompt_multiple_files`
- ✅ `load_developer_project_prompt_skips_empty_files`
- ✅ `load_developer_project_prompt_missing_file_skipped`
- ✅ `load_developer_project_prompt_all_missing_returns_none`
- ✅ `from_config_includes_developer_project_prompt_section`
- ✅ `from_config_developer_prompt_before_project_context`
- ✅ `from_config_developer_prompt_with_preloaded_content`
- ✅ `developer_prompt_section_heading`

---

## 三、会话链路接入 (Plan 3) ✅

### 3.1 启动期 (Bootstrap)
- **文件**: `crates/nova-agent/src/app/bootstrap.rs:95-99`
- **状态**: ✅ 启动期保存文件列表，不主动读取项目根文件

### 3.2 会话执行期 (ConversationService)
- **文件**: `crates/nova-agent/src/app/conversation_service.rs:295-307`
- **状态**: ✅ 在每轮开始前加载，有日志记录命中文件数量

### 3.3 Prompt Reload
- **文件**: `crates/nova-agent/src/app/agent_workspace_service.rs:142-156`
- **状态**: ✅ reload 走同一套逻辑，重新加载配置文件

---

## 四、约束实现检查

| 约束 | 实现 | 状态 |
|------|------|------|
| 不做长度上限 | `load_developer_project_prompt` 无长度限制 | ✅ |
| 统一 append | `SystemPromptBuilder::from_config` 统一 append | ✅ |
| 不做目录递归 | `project_dir.join(file_name)` 仅根目录 | ✅ |
| 整文件合并 | `tokio::fs::read_to_string` 整文件读取 | ✅ |

---

## 五、问题与改进建议

### 5.1 ⚠️ from_config 中的双重加载
**文件**: `crates/nova-agent/src/prompt.rs:804-810`

```rust
// L4: 开发项目提示词（在 ProjectContext 之前）
if let Some(content) = &config.developer_project_prompt_content {
    builder = builder.developer_project_prompt_section(content);
} else if let Some(content) =
    load_developer_project_prompt(config.project_dir.as_deref(), &config.developer_prompt_files)
{
    builder = builder.developer_project_prompt_section(&content);
}
```

**问题**: `from_config` 会先检查 `developer_project_prompt_content`，如果没有则重新加载。但在 `ConversationService` 和 `reload_session_system_prompt` 中已经预加载了内容并设置到 `PromptConfig` 中。

**影响**: 如果 `developer_project_prompt_content` 为 `None`，会触发同步加载（`load_developer_project_prompt`），而不是使用异步版本。这可能导致在 `from_config` 被同步调用时有轻微的 I/O 阻塞。

**建议**: 考虑在 `from_config` 中使用异步版本或使用预加载内容。

---

### 5.2 ⚠️ 拼装顺序注释标记
**文件**: `crates/nova-agent/src/prompt.rs:781-839`

```rust
// L0: 平台身份（agent prompt 文件内容，经模板替换）
// L1: 行为约束
// L2: Skills（按需注入）
// L4: 开发项目提示词（在 ProjectContext 之前）
// L5: 项目上下文
// L6: 环境快照
```

**问题**: 注释标记为 L4/L5/L6，但实际是 L3/L4/L5（Skill 是 L2）。

**建议**: 统一注释标记，或移除 L 前缀直接使用中文描述。

---

### 5.3 💡 可选增强：来源可观测性
**设计文档提到**: "建议在合并文本中保留 `Source:` 标记" 和 "必要时可补充 `developer_project_prompt_sources: Vec<PathBuf>`"

**当前状态**: 
- ✅ 合并文本中已保留 `Source:` 标记
- ⚠️ `developer_project_prompt_sources` 未实现（设计文档中提到可作为次要增强项）

**建议**: 后续可考虑添加 `sources` 字段用于调试和 preview。

---

## 六、总结

**整体评价**: 代码实现与设计文档高度一致，覆盖了所有核心功能。

**已正确实现的部分**:
1. ✅ 配置模型扩展（AppConfig, AgentSpec, PromptConfig）
2. ✅ 提示词加载与拼装（加载函数、SectionName、拼装顺序）
3. ✅ 会话链路接入（启动期、会话执行期、reload）
4. ✅ 错误处理（单文件失败不影响整体）
5. ✅ 测试覆盖（10+ 单元测试）

**可能需要关注的部分**:
1. ⚠️ `from_config` 中的双重加载逻辑（同步 vs 异步）
2. ⚠️ 拼装顺序注释标记

**无显著遗漏**。

---

## 七、相关文件清单

| 文件 | 职责 |
|------|------|
| `crates/nova-agent/src/config.rs` | 配置模型（AppConfig, AgentSpec, PromptConfig） |
| `crates/nova-agent/src/prompt.rs` | 提示词加载与拼装（加载函数、SectionName、SystemPromptBuilder） |
| `crates/nova-agent/src/app/conversation_service.rs` | 会话执行期加载 |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 会话 reload |
| `crates/nova-agent/src/app/bootstrap.rs` | 启动期配置 |
