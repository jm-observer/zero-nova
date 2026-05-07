# Plan 3: Shell 调用方式统一与回归测试

## 前置依赖
Plan 1、Plan 2

## 本次目标
统一工具执行的 shell 调用策略，消除跨 shell 嵌套带来的转义和编码风险，并通过自动化测试固化行为。

## 涉及文件
- `crates/*/src/*shell*.rs` — shell 选择与命令封装
- `crates/*/src/*tool_runner*.rs` — 工具运行入口
- `crates/*/tests/*tool*` — 集成测试
- `docs/` 下运行规范文档（如已有）— 调用约束更新

## 详细设计

### 1. 单一原生 shell 原则

按平台选择一个原生 shell 并直接执行：
- Windows：直接使用 PowerShell 执行，不再通过 Bash 包裹。
- Linux：优先直接使用 `bash -lc`（不可用时回退 `sh -lc`），并记录实际 shell。
- macOS：直接使用 Bash 或 `sh`（按现有实现保持一致）。

禁止模式：
- Bash -> PowerShell
- PowerShell -> Bash

### 2. 命令封装规范

- 参数与命令分离，尽量使用结构化参数而非整段字符串拼接。
- 统一转义工具，避免多层手工转义。
- 对高风险字符做最小必要转义并增加测试样例。

### 3. 观测与诊断增强

每次执行记录结构化字段：
- `shell_type`
- `os_type`
- `locale`（`LANG`、`LC_ALL`）
- `command_digest`（摘要，不记录敏感全文）
- `exit_code`
- `stderr_encoding`
- `stdout_encoding`

这样可快速定位“是参数问题、shell 问题，还是编码问题”。

### 4. 回归测试矩阵

至少覆盖以下组合：
1. Windows + PowerShell + UTF-8 输出
2. Windows + PowerShell + 非 UTF-8 stderr
3. Linux + bash/sh + UTF-8 locale
4. Linux + `LANG=C` 或 `LC_ALL=C` + 非 UTF-8 stderr/stdout
5. 参数非法 + 不启动进程
6. 非 0 退出 + stderr 可读
7. 历史问题命令模板（跨 shell 嵌套）应被拒绝或重写

## 测试案例

1. **禁止跨 shell 嵌套**：输入嵌套模板时，校验阶段直接报错。
2. **原生 shell 成功执行**：返回完整 stdout/stderr 与退出码。
3. **失败命令可诊断**：stderr 编码、bytes 长度、shell 类型均可见。
4. **安全回归**：确保命令封装后不会引入参数注入回归。
5. **端到端回归**：复现最初 `stream did not contain valid UTF-8` 场景，确认不再出现。
6. **Linux 端到端回归**：在容器或 CI Linux runner 中复现 locale 差异，确认 stdio 编码诊断字段完整。
