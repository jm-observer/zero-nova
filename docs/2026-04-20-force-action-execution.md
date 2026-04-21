# 设计文档：强化 Agent 行为执行力 (Force Action Execution)

- **时间**：2026-04-20
- **项目现状**：Agent 加载长指令 Skill 后倾向于“言语承诺”而非“物理执行”，导致 `tool_use` 节点缺失。
- **本次目标**：在 System Prompt 末尾注入行为护栏，强制模型将言论与工具调用对齐。

## 详细设计

### 1. 修改逻辑
在 `src/gateway/mod.rs` 的 Agent 初始化环节，为每个 Agent 的 `full_system_prompt` 拼接一组 `BEHAVIOR_GUARDS`。

### 2. 注入内容 (BEHAVIOR_GUARDS)
```markdown
## CRITICAL: Action Consistency
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
```

## 测试案例
1. **测试场景**：触发 `skill-creator` 重新运行测试。
2. **预期结果**：模型回复中必须包含 `bash` 或 `write_file` 的工具调用块，而不仅仅是文本。
