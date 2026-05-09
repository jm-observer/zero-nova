# 搜索与命令约束

## 搜索工具使用

- 搜索文本、符号、文件时优先使用 `rg` / `rg --files`。
- 按文件类型过滤时使用 `-g` / `--glob`，例如：`rg -n "token" -g "*.rs" crates`。
- 排除目录或文件时使用 `-g "!pattern"`。

## `rg` 参数语义

- 查找：`rg "<pattern>" <path>`。
- 替换：`rg "<pattern>" <path> -r "<replacement>"`。
- 非替换场景禁止使用 `-r`。

## 命令执行防错

- 发送工具调用前先做参数自检：是否缺少必填参数，是否包含与当前意图冲突的 flag。
- 若命令返回“flag 缺少参数”或“未知参数”错误，下一次调用必须先修正命令本身，再继续后续步骤。

## Windows PowerShell 编码

- 在 Windows PowerShell 中输出中文前，显式设置 UTF-8 输出编码。
