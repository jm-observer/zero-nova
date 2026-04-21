# Skill Creator Timeout Debug

## 时间

- 创建日期：2026-04-21
- 最后更新：2026-04-21

## 项目现状

`skill-creator` 已适配 zero-nova 的本地技能目录和 `nova_cli run --json` 调用方式。描述优化流程通过 `.nova/skills/skill-creator/scripts/run_loop.py` 启动多轮触发评估，并在每个 eval query 中调用 `cargo run --bin nova_cli -- run ... --json`。

本次分析的 `target/request` 显示，优化流程曾多次进入 `Iteration 1/5`，但外层工具调用以 `Command timed out after 30000ms` 结束。随后出现多个遗留的 `cargo run --bin nova_cli` / `nova_cli.exe` 进程，继续占用资源并覆盖全局 `target/request`。

## 本次目标

1. 让 `skill-creator` 的子进程超时后能够清理完整进程树，避免遗留 `nova_cli.exe`。
2. 明确区分外层工具超时参数 `timeout_ms` 和 `run_loop.py` 的 `--timeout` 参数，避免后续误用。
3. 记录 `target/request` 分析中发现的问题、判断依据和修复方法。

## 详细设计

### 问题 1：外层工具超时和脚本参数混淆

现象：

- `target/request` 中多次出现 `Command timed out after 30000ms`。
- 命令字符串中曾尝试添加 `--timeout_ms 1200000`，但 `run_loop.py` 不支持该参数。
- 后续改成 `--timeout 1200` / `--timeout 3600` 后仍然 30 秒超时。

原因：

- `timeout_ms` 是工具调用层的超时字段，单位毫秒，必须作为 tool call JSON 的平级字段传入。
- `--timeout` 是 `run_loop.py` 的参数，表示每个 query 子进程允许运行的秒数。
- 把 `--timeout_ms` 写进 shell command 字符串不会影响外层工具超时。

解决：

- 更新 `.nova/skills/skill-creator/SKILL.md`，明确要求工具调用写成 `{"command": "...", "timeout_ms": 600000}`。
- 文档中明确：遇到 `Command timed out after 30000ms` 时，调整 tool call 的 `timeout_ms`，不是改 `run_loop.py --timeout`。

### 问题 2：超时后遗留子进程

现象：

- 外层调用超时后，仍能查到多个 `cargo run --bin nova_cli` 和 `nova_cli.exe` 进程。
- 这些进程的命令行仍在执行 eval query，例如 message queue 或 sorting algorithms 相关测试。

原因：

- Python 中原先只对直接 `Popen` 出来的 `cargo` 进程调用 `process.kill()`。
- Windows 上 `cargo` 再启动 `nova_cli.exe`，杀父进程不保证杀掉子进程。
- 遗留子进程继续写全局 `target/request`，导致后续分析容易误判。

解决：

- 在 `.nova/skills/skill-creator/scripts/utils.py` 中增加 `subprocess_group_kwargs()` 和 `terminate_process_tree()`。
- Windows 下使用 `taskkill /PID <pid> /T /F` 清理整棵进程树。
- 非 Windows 下使用独立 process group，并依次发送 `SIGTERM` / `SIGKILL`。
- `run_eval.py` 和 `improve_description.py` 都改为使用该工具清理超时子进程。

### 问题 3：`target/request` 不是稳定的 eval 证据

现象：

- `target/request` 只保存全局最后一次模型请求。
- 外层对话、eval 子进程、遗留 `nova_cli` 都可能覆盖它。
- `target/response` 的时间戳可能和 `target/request` 不匹配。

原因：

- `target/request` 是全局调试快照，不是每个 eval run 的独立产物。
- 并发或遗留进程会让该文件指向最后一个写入者，而不是用户正在分析的 run。

解决：

- `run_eval.py` 已在每个 run 的输出目录中复制 `request.json` / `response.json`，用于后续逐个分析。
- 当外层工具超时后，必须先确认是否存在 `results.json`、`report.html`、`logs/` 或仍在运行的进程，再汇报状态。
- 不得仅凭全局 `target/request` 判断某个 eval 是否触发了临时 skill。

### 问题 4：错误归因漂移

本次历史中出现过多个不同错误：

- eval schema 错误：`TypeError: string indices must be integers`、`KeyError: 'query'`。
- 外层工具默认 30 秒超时：`Command timed out after 30000ms`。
- 本地鉴权错误：`401 Unauthorized`。
- 早期 Windows 编码问题：`UnicodeDecodeError`。

解决：

- 每次分析必须以当前 `target/request` 的时间戳和最后几条消息为准。
- 不同时间点的 `target/request` 和 `target/response` 不能强行配对。
- 报告中要区分“本次请求的直接失败原因”和“历史上曾出现的问题”。

## 测试案例

1. 正常路径：运行 `run_eval.py`，子进程在 timeout 内返回 JSON，输出目录包含 `result.json`，进程正常退出。
2. 超时路径：将 query timeout 设置为很小，确认 `run_eval.py` 返回 `timed_out: true`，且没有遗留 `cargo run --bin nova_cli` / `nova_cli.exe`。
3. 改进描述超时：让 `improve_description.py` 的 `_call_claude` 超时，确认抛出明确的 `TimeoutError` 并清理进程树。
4. 文档路径：检查 `SKILL.md` 中的无人值守模式说明，确认没有建议把 `--timeout_ms` 写入 shell command。

## 风险与待定项

- `taskkill /T /F` 会强制结束指定进程树。当前只用于脚本自己启动的 `cargo`/`nova_cli` 进程，避免匹配系统中无关进程。
- `target/request` 仍是全局文件。更稳妥的长期方案是让 `nova_cli` 支持按 run 输出 debug request/response 到指定目录，而不是由外部脚本复制全局快照。
- 如果外层工具本身强制中断 Python 进程，Python 的 `finally` 不一定有机会运行。仍需在再次执行前检查并清理遗留进程。
