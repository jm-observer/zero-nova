# Nova CLI 特性分析与 Skill Creator 适配路线图

| 时间 | 2026-04-22 |
| :--- | :--- |
| **项目现状** | 已有初步的 `nova-cli` 实现，但尚未完全支持 `skill-creator` 的自动化评测与优化流。 |
| **对比目标** | `nova-cli` 现有能力 vs `skill-creator` 脚本需求。 |

---

## 1. `nova-cli` 现有特性 (Plan 4 已定义)

1.  **交互模式 (`chat`)**: 支持 REPL，具备 `/tools`, `/clear`, `/mcp` 等斜杠命令。
2.  **单轮执行 (`run`)**: 适用于脚本集成的 One-shot 模式，返回状态码。
3.  **内置工具集**: 已集成 `web_search`, `web_fetch`, `bash`, `read_file`, `write_file`。
4.  **MCP 支持**: 支持动态注入 MCP Server 扩展工具集。
5.  **流式事件渲染**: 能将 `AgentEvent` (TextDelta, ToolStart/End) 渲染到 stdout。

---

## 2. 缺失的关键特性 (为了支持 Skill Creator)

为了让 `run_eval.py` 和 `run_loop.py` 能够无缝切换到 `nova-cli`，需要补充以下能力：

### 2.1 结构化 JSON 输出模式 (`--output-format stream-json`)
*   **需求**: `run_eval.py` 依赖实时监听 `tool_use` 事件来判断是否“触发了技能”。目前 `nova-cli` 主要输出为人眼可读的文本。
*   **适配**: 需增加 `--output-format stream-json` 参数，将内部 `AgentEvent` 序列化为单行 JSON 流输出，方便 Python 脚本通过 `json.loads` 实时解析。

### 2.2 技能注入机制 (`--include-skill <path>`)
*   **需求**: `skill-creator` 必须能让子代理带着某个特定 Skill 启动。
*   **现状**: `nova-cli` 目前仅加载 Built-in 和通过 `/mcp` 注入的工具，不支持加载 `SKILL.md` 的指令集。
*   **适配**: 需增加 `--include-skill` 参数，读取指定路径的 `SKILL.md` 并将其指令注入到 System Prompt 的开头或 `available_skills` 段落中。

### 2.3 工作目录深度隔离 (`--workspace <path>`)
*   **需求**: 为了解决之前发现的“沙箱隔离问题”，CLI 应支持显式指定工作目录。
*   **适配**: 启动时自动切换 `CWD` 到指定的 workspace，并限制 `read_file` / `write_file` 的操作范围在该目录下。

### 2.4 扩展元数据流
*   **需求**: `run_eval.py` 需要统计 Token 消耗和执行时长。
*   **适配**: 确保 `TurnComplete` 事件中包含详尽的 `usage` (Token) 和 `duration_ms` 数据，并以 JSON 形式输出。

---

## 3. 待改造的方法映射表

| 脚本/方法 | 原逻辑 (Claude Code) | 改造逻辑 (Nova CLI) |
| :--- | :--- | :--- |
| `run_eval.py / find_project_root` | 寻找 `.claude` | 寻找 `.nova` 或项目根目录 |
| `run_eval.py / run_eval` | 调用 `claude -p` | 调用 `nova-cli run --output-format stream-json` |
| `run_eval.py / result_parsing` | 过滤特定 stream 事件 | 对接 `AgentEvent` 的 JSON 表征 |
| `improve_description.py / call_llm` | 调用交互式 LLM | 调用 `nova-cli run` 并捕获最终文本输出 |

---

## 4. 后续步骤计划
1.  **修改 `src/bin/nova_cli.rs`**: 增加 `--output-format` 和 `--include-skill` 参数支持。
2.  **重写 `run_eval.py` 的事件解析逻辑**: 使其能够识别 `nova-cli` 的工具调用 JSON 标记。
3.  **验证**: 使用 `tech-solution-architect` 重新跑一次 `run_loop.py`，确认隔离问题和触发识别问题已修复。
