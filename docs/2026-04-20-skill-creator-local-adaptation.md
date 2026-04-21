# 2026-04-20-skill-creator-local-adaptation

| 章节 | 说明 |
|-----------|------|
| 时间 | 2026-04-20 |
| 项目现状 | `skill-creator` 脚本硬编码调用 `claude` CLI，无法在本地大模型环境直接运行。`nova_cli` 目前支持交互式交互，但缺乏为脚本设计的结构化输出模式。 |
| 本次目标 | 适配 `skill-creator` 脚本，使其能够通过 `nova_cli` 调用本地 LLM 进行技能触发测试与迭代，实现完全本地化的技能开发闭环。 |
| 详细设计 | **1. `nova_cli` 增强：**<br>- 扩展 `Run` 子命令，增加 `--json` 标志。<br>- 当启用 `--json` 时，不打印彩色格式文字，而是将 `TurnResult` 以 JSON 格式输出到 stdout。<br>- 确保 `Run` 命令也会加载 `.nova/skills` 目录下的技能。<br><br>**2. `skill-creator` 脚本修改：**<br>- 在 `scripts/utils.py` 或 `run_eval.py` 中引入配置项，用于指定使用的 CLI 工具路径（默认指向 `nova_cli`）。<br>- 修改 `run_single_query` 函数：<br>  - 命令由 `claude -p` 改为 `cargo run --bin nova_cli -- run --json`。<br>  - 适配解析逻辑：解析 `nova_cli` 输出的 JSON 以提取工具调用（ToolUse）事件。<br>  - 注意：由于 `zero-nova` 会把 Skill Body 注入 System Prompt，我们不再需要往 `.claude/commands` 写临时文件，而是直接依据 System Prompt 触发。 |
| 测试案例 | 1. 运行 `cargo run --bin nova_cli -- run "hello" --json` 验证是否有有效的 JSON 输出。<br>2. 运行 `skill-creator/scripts/run_eval.py` 验证是否能成功调用本地 `nova_cli` 并判定技能触发状态。 |
| 风险与待定项 | 1. 性能：通过 `cargo run` 调用开销较大，生产环境下建议使用编译后的二进制路径。<br>2. 协议匹配：`nova_cli` 的 JSON 输出格式需与脚本预期的 `stream-json` 尽可能保持语义一致或在脚本侧进行适配。 |
