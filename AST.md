## 使用 `ast-outline` 优先理解代码结构

在阅读仓库代码时，优先使用 `ast-outline` 获取结构化信息，避免一开始就完整读取大文件。只有当结构、签名或目标符号内容不足以回答问题时，才回退到完整读取源文件。

### 核心原则

- 先看结构，再读实现。
- 只提取当前任务真正需要的方法、类型或文档章节。
- 遇到陌生目录，先生成模块概览。
- 重构前先查看依赖关系和影响范围。
- 如果 `ast-outline` 输出包含解析错误警告，应直接读取受影响区域的源码确认。

### 常用命令

- `rg "<pattern>" <path>`
    - 默认用于文本搜索；不要附带 `-r`。
    - `-r/--replace` 仅用于“搜索并替换”场景，且必须显式提供替换字符串，例如 `rg "<pattern>" <path> -r "<replacement>"`。
    - 仅做“查找引用/定位用法”时，禁止使用 `-r`，避免触发 `missing value for flag -r`。

- `ast-outline digest <dir>`
    - 生成目录的一页式模块地图。
    - 适合首次接触陌生目录，快速了解每个文件中的类型和公开方法。

- `ast-outline outline <file>`
    - 查看单个文件的结构、签名和行号范围，不包含方法体。
    - 适合判断文件形状、定位目标符号，通常比完整读取小很多。

- `ast-outline show <file> <Symbol>`
    - 提取指定方法、类、类型或 Markdown 标题章节的源码。
    - 支持后缀匹配；有歧义时使用 `Type.Symbol`。
    - 可一次提取多个符号。

- `ast-outline implements <Type> <dir>`
    - 查找某个类型的实现、子类或继承关系。
    - 默认包含传递匹配，并用 `[via Parent]` 标注间接来源。
    - 使用 `--direct` 仅查看一级实现。

- `ast-outline search "<query>"`
    - 在仓库中进行混合 BM25 与语义搜索。
    - 使用标识符查询符号，例如 `HandlerStack`。
    - 使用自然语言查询行为，例如 `"how does login work"`。
    - 首次调用会在 `.ast-outline/index/` 建立索引，后续增量复用。

- `ast-outline find-related <file>:<line>`
    - 查找与指定文件行所在代码块语义相似的其他代码块。
    - 适合寻找相似实现、替代方案或重复模式。
    - 可直接使用 `search` 输出中的 `path:start-end` 形式。

- `ast-outline surface <dir>`
    - 查看包的真实公开 API。
    - Rust 会解析 `pub use`，Python 会解析 `__all__`。
    - 使用 `--tree` 查看层级结构。
    - 使用 `--include-chain` 查看 re-export 路径。

### 依赖图命令

- `ast-outline deps <file> [--depth N]`
    - 查看文件直接或间接导入了哪些文件。

- `ast-outline reverse-deps <file> [--depth N]`
    - 查看哪些文件依赖当前文件。
    - 重构、移动或删除文件前应优先使用，用于评估影响范围。

- `ast-outline cycles [<dir>]`
    - 检查导入循环，基于 Tarjan SCC。
    - 存在循环时返回非零状态，适合作为 CI 检查。

- `ast-outline graph [<dir>] --format text|json|dot|dsm`
    - 输出完整依赖图。
    - `dsm` 格式可用于观察设计结构矩阵，帮助发现循环和依赖反转问题。

### 输出选项

- 大多数命令支持 `--json`，用于稳定结构化输出。
- 大多数命令支持 `--compact`，用于单行 JSON 输出。
- 不传命令或传入未知参数时，会显示帮助文本。
- `ast-outline` 没有默认命令，每次操作都必须显式指定子命令。

### 推荐工作流

1. 面对陌生目录：先运行 `ast-outline digest <dir>`。
2. 面对单个文件：先运行 `ast-outline outline <file>`。
3. 需要具体实现：再运行 `ast-outline show <file> <Symbol>`。
4. 不知道文件或符号名：运行 `ast-outline search "<query>"`。
5. 需要找相似代码：运行 `ast-outline find-related <file>:<line>`。
6. 需要确认公开 API：运行 `ast-outline surface <dir>`。
7. 重构前：运行 `ast-outline reverse-deps <file>` 评估影响范围。
8. 排查架构问题：运行 `ast-outline cycles` 或 `ast-outline graph`。

### 回退规则

仅在以下情况完整读取源文件：

- `outline` 或 `show` 提供的信息不足。
- 需要理解符号周边上下文。
- `ast-outline` 输出提示解析错误。
- 目标内容不是 `ast-outline` 能准确提取的结构。
