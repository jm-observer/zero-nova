
## 命令兼容性约束（ripgrep）
- 使用 `rg`（ripgrep）按文件类型过滤时，必须使用 `-g` / `--glob`，例如：`rg -n "token" -g "*.ts" deskapp/src`。
- 禁止生成 `rg --include "*.ts" ...` 这类命令；`--include` 不是 ripgrep 的有效参数，会报 `unrecognized flag --include`。
- 若需排除目录/文件，使用 `-g "!pattern"`，例如：`-g "!node_modules/**"`。
- 在不确定参数时，先生成最小可执行命令（如 `rg -n "kw" path`），再逐步增加 `-g` 约束，避免一次性拼接无效参数。
