# Phase 6：Skill 接入、历史管理与稳定性收口

> 前置依赖：Phase 1-5  
> 相关设计：`docs/skill-system-design.md`、`docs/conversation-control-plane-design.md`

## 1. 目标

第六阶段才处理 skill。  
原因很简单：在当前 `src` 基础上，**skill 不是顶层问题**，控制层才是。  
等 session、chat 生命周期、control plane、workflow、multi-agent 都稳定后，再把 skill 作为执行层能力接进来，成本最低。

本阶段目标：

- skill 定义与加载
- skill prompt 注入
- tool 白名单过滤
- 历史消息摘要压缩
- 日志与稳定性收口

## 2. 本 phase 范围

### 2.1 要做

- 接入 `src/skill/*`
- 调整 `prompt.rs`
- 调整 `tool.rs`
- 引入 skill history 管理
- 完善日志与回归测试

### 2.2 不做

- 不把 skill 提升回顶层入口
- 不做过度复杂的 skill 组合执行

## 3. 设计结论

### 3.1 skill 是执行层，不是控制层

最终分层应是：

- control plane
- workflow / agent context
- skill
- tool execution

不要反过来让 skill 再次决定顶层输入流向。

### 3.2 `SystemPromptBuilder` 要从“全量工具静态拼接”升级成“按 turn 组装”

当前 `src/prompt.rs` 的问题是：

- 默认 prompt 固定
- `.with_tools(&registry)` 会把全部工具描述塞进去

Phase 6 需要改成：

- base prompt
- skill prompt（可选）
- environment info（可选）
- filtered tools

### 3.3 `ToolRegistry` 需要支持过滤

当前只有：

- `tool_definitions()`
- `execute(name, input)`

需要补：

- `tool_definitions_filtered(...)`

以便 skill 或 workflow 限制工具暴露面。

## 4. 实现细节

### 4.1 接入 skill 模块

在当前 `src` 结构上，skill 相关代码应作为新增模块接入，而不是侵入 `gateway`。

### 4.2 引入按 turn 的上下文对象

建议在 runtime 层引入：

```rust
pub struct TurnContext {
    pub system_prompt: String,
    pub tool_definitions: Vec<ToolDefinition>,
    pub history: Vec<Message>,
    pub active_skill: Option<String>,
}
```

这样 `AgentRuntime::run_turn()` 才能摆脱“固定 prompt + 全量工具”的限制。

### 4.3 引入历史压缩

当 skill 或 workflow 上下文发生明显切换时，对旧历史做规则摘要，而不是永远把全量 history 带给模型。

## 5. 稳定性收口

这一阶段要补：

- 更完整日志
- 更清晰错误码
- timeout / cancel 回归测试
- protocol / control plane / skill 的组合测试

## 6. 测试方案

### 6.1 Skill 测试

覆盖：

- 目录加载
- prompt 注入
- 工具过滤
- skill fallback

### 6.2 历史测试

覆盖：

- 同 skill 下完整保留
- 切 skill 后触发摘要
- 摘要保留必要上下文

### 6.3 全量回归

命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 7. 风险点

### 7.1 在控制层未稳定前先接 skill

这会导致 skill 承担本不该承担的顶层职责，最终还是要返工。

### 7.2 历史压缩做得过早或过重

先做规则摘要，不要一开始就引入昂贵 LLM 摘要路径。

## 8. 完成定义

- skill 已作为执行层能力接入
- prompt 和工具按 turn 动态组装
- 历史压缩可工作
- 系统进入相对稳定可扩展状态

## 9. 最终交付判断

当 Phase 6 完成时，系统应具备：

- 稳定后端网关
- 可控会话模型
- control plane
- workflow
- multi-agent
- skill 执行层

这时再继续扩展新 agent / 新 workflow / 新 skill，成本才是线性的，而不是继续堆 patch。
