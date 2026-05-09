
## 命令兼容性约束（ripgrep）
- 使用 `rg`（ripgrep）按文件类型过滤时，必须使用 `-g` / `--glob`，例如：`rg -n "token" -g "*.ts" deskapp/src`。
- 禁止生成 `rg --include "*.ts" ...` 这类命令；`--include` 不是 ripgrep 的有效参数，会报 `unrecognized flag --include`。
- 若需排除目录/文件，使用 `-g "!pattern"`，例如：`-g "!node_modules/**"`。
- 在不确定参数时，先生成最小可执行命令（如 `rg -n "kw" path`），再逐步增加 `-g` 约束，避免一次性拼接无效参数。

## Windows PowerShell 输出编码约束
- 在 Windows PowerShell 中执行包含中文输出的命令前，应显式设置 UTF-8 输出编码：`[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $OutputEncoding = [System.Text.UTF8Encoding]::new()`。
- 避免将原生命令输出直接接入可能重编码文本的 PowerShell 管道（如 `rg ... | Select-Object -First 50`）；需要限制结果数量时，优先使用工具自身参数或纯文本方式，例如 `rg -n "kw" path -m 50`。
- 看到 `stdout_encoding: lossy` 或输出中出现 `�` 时，禁止直接判断源码文件乱码；必须先用显式 UTF-8 重新读取目标文件或重新执行命令验证。
- 排查编码问题时，应区分“文件内容编码”和“终端/工具捕获 stdout 的解码”；只有在 UTF-8 读取文件本身也异常时，才能认为文件内容可能损坏。
