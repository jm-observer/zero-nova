# Plan 1: stderr 编码采集与解码链路修复

## 前置依赖
无

## 本次目标
修复子进程 stdio（stdout/stderr）的编码链路，保证即使输出非 UTF-8 字节也能被稳定读取、记录和返回，避免二次报错掩盖原始错误。

## 涉及文件
- `crates/*/src/*process*.rs`（实际进程执行模块）— 子进程 stdout/stderr 读取策略
- `crates/*/src/*tool*.rs`（实际工具执行模块）— 执行结果结构与错误返回
- `crates/*/src/*error*.rs`（错误模型）— stderr 解码失败的错误语义

## 详细设计

### 1. 读取策略从“强 UTF-8”改为“字节优先 + 容错文本”

执行器读取 stdout/stderr 时分别保留两份数据：
1. `*_bytes: Vec<u8>`：原始字节，作为真实来源。
2. `*_text: String`：展示用文本，按以下规则生成：
- 优先尝试 UTF-8 严格解码。
- 失败后使用 `String::from_utf8_lossy` 容错解码。

这样即使 stderr/stdout 包含本地代码页字节，也不会导致链路直接失败。

### 2. 结果模型扩展

在工具执行结果中补充字段：
- `stderr` / `stdout`：容错后的字符串（现有字段保留）
- `stderr_encoding` / `stdout_encoding`：`"utf8" | "lossy"`
- `stderr_bytes_len` / `stdout_bytes_len`：原始字节长度

如需控制响应体大小，可仅返回长度和截断后的文本，完整字节写入调试日志。

### 3. 子进程环境统一 UTF-8（尽力而非强依赖）

在进程启动时注入 UTF-8 相关环境变量或 shell 初始化设置（按平台区分），例如：
- Windows PowerShell：在命令前设置输出编码为 UTF-8。
- Linux/macOS：设置或继承 UTF-8 locale（如 `LANG` / `LC_ALL`），并记录生效值用于诊断。

注意：该步骤是“提高概率”的优化，不能替代读取侧容错。

### 4. 错误模型调整

原先“stderr 非 UTF-8”不再作为致命错误；致命错误应回归到：
- 进程启动失败
- 命令执行失败（非 0 退出）
- I/O 读取失败

编码问题仅作为 `stderr_encoding/stdout_encoding = lossy` 的诊断信号。

### 5. 日志边界

避免多层重复打印同一 stderr/stdout。
- 执行层记录一次结构化日志（exit code、stdio encoding、stdio bytes len、平台与 shell）。
- 上层仅透传，不重复打印全文。

## 测试案例

1. **正常 UTF-8 stderr**：应返回 `stderr_encoding = "utf8"`，文本无替换字符。
2. **非 UTF-8 stderr 字节**：命令退出非 0 时仍能返回错误文本，`stderr_encoding = "lossy"`。
3. **空 stderr**：应返回空字符串，`stderr_bytes_len = 0`。
4. **大体积 stderr**：验证截断策略与长度统计一致。
5. **Windows 本地代码页场景**：模拟 CP936 字节输出，确认链路不再抛 UTF-8 解码异常。
6. **Linux 非 UTF-8/混合 locale 场景**：覆盖 `LANG=C`、`LC_ALL=C` 等条件，确认 stdout/stderr 仍可容错返回。
