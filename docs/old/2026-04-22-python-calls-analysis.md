# Skill Creator 全流程工具调用与执行链分析

| 时间 | 2026-04-22 |
| :--- | :--- |
| **项目现状** | 正在分析 `tech-solution-architect` Skill 的创建与测试全过程。 |
| **数据源** | `target/request` (日志记录) |

---

## 1. 基础设施准备 (Infrastructure)

在 Skill 创建初期，主代理通过核心工具建立目录结构并初始化主要资源。

### 1.1 目录初始化 (`bash`)
*   **用途**: 建立 Skill 及其测试工作区。
*   **调用内容**:
    ```bash
    mkdir -p tech-solution-architect/evals
    mkdir -p tech-solution-architect/workspace
    ```

### 1.2 技能定义初始化 (`write_file`)
*   **用途**: 写入 `SKILL.md`，定义 Skill 的核心逻辑与约束。
*   **关键内容**: 定义了三阶段工作流（调研方案、环境适配、部署测试），并强制要求中文输出。

### 1.3 测试集初始化 (`write_file`)
*   **路径**: `tech-solution-architect/evals/evals.json`
*   **调用内容**: 写入 3 个测试用例（实时日志系统、轻量键值存储、API 网关）。

---

## 2. 自动化评测执行链 (Evaluation Loop)

### 2.1 工作区深度预备 (`bash`)
*   **用途**: 为每一个 Eval Case（eval-0, eval-1, eval-2）分别建立 `with_skill` 和 `without_skill` 两个子目录。
*   **调用**: `mkdir -p tech-solution-architect-workspace/iteration-1/eval-N/...`

### 2.2 隔离代理启动 (`spawn_subagent`)
这是整个流程中最关键的环节，也是目前发现问题的根源。

#### A. 加载技能的调用 (With Skill)
*   **示例参数**:
    ```json
    {
      "system_prompt_patch": "You are an expert technical architect. Use the following skill to help the user.\nPath to skill: ./tech-solution-architect/SKILL.md",
      "task": "Execute this task:\n- Skill path: ./tech-solution-architect/SKILL.md\n- Task: 我想了解一下如何搭建一个高性能的、开源的实时日志分析系统...",
      "workspace": "/tmp/tech-eval-0-with"
    }
    ```
*   **执行结果**: ⚠️ **失败**。子代理报错 `Access denied: path is outside of allowed workspace`。
*   **错误分析**: 子代理试图访问绝对路径或主代理进程目录下的 Skill 文件，被沙箱机制阻断。

#### B. 基准测试调用 (Without Skill)
*   **执行结果**: ✅ **成功**。虽然脱离了 Skill 指令，但子代理通过自身的知识库生成了方案文档（例如第 134 行生成的日志分析架构建议书）。

---

## 3. 观察到的异常行为链

在子代理由于 **2.2.A** 失败后，触发了以下非预期的工具调用流：

1.  **盲目文件读取**: `read_file` 尝试直接读取 `./tech-solution-architect/SKILL.md` (失败)。
2.  **暴力递归查找 (`bash`)**:
    *   `ls -R .` (在空目录中查找)
    *   `find . -maxdepth 3 -not -path '*/.*'`
    *   `find . -name "SKILL.md"`
    *   `find / -name "SKILL.md"` (试图扫描整个根目录)
3.  **Token/时间损耗**: 由于不断通过 `bash` 进行文件搜索尝试，单次子代理调用耗时超过 40 秒，消耗 Token 超过 1.2 万。

---

## 4. 核心脚本功能与执行节点分析

`skill-creator` 的自动化链路主要由 `scripts/` 目录下的 Python 脚本驱动。以下是各脚本的详细解析：

### 4.1 流程编排脚本 (`run_loop.py`)
*   **执行节点**: 整个优化生命周期的“指挥官”。
*   **核心功能**: 
    1. 自动将 Eval Set 划分为 **Train** (训练集) 和 **Test** (验证集)。
    2. 循环调用 `run_eval.py` 获取当前描述的分数。
    3. 如果未全通过，则调用 `improve_description.py` 优化描述。
    4. 持续更新 `generate_report.py` 实时报告。
*   **关键参数**:
    *   `--max-iterations`: 最大迭代次数（默认 5）。
    *   `--holdout`: 验证集比例（默认 0.4）。
    *   `--runs-per-query`: 每个查询跑几次（默认 3）以计算触发率。

### 4.2 触发评测脚本 (`run_eval.py`)
*   **执行节点**: 被 `run_loop.py` 频繁调用。
*   **核心功能**: 并行启动多个工作线程，使用 `subprocess` 调用 `claude -p` (CLI) 来验证在给定描述下，模型是否会触发该技能。
*   **参数**: `--skill-name`, `--description`, `--num-workers`。

### 4.3 描述改进脚本 (`improve_description.py`)
*   **执行节点**: 在 `run_eval.py` 之后、下一轮评估之前。
*   **核心功能**: 构造一段复杂的 Prompt 发送给 LLM。Prompt 中包含了当前描述、失败的 Query、以及历史改进记录。它要求模型分析“漏触发”或“误触发”的原因，并提出更精准的描述。

### 4.4 报告生成脚本 (`generate_report.py`)
*   **执行节点**: 运行中和运行结束后实时调用。
*   **核心功能**: 将优化过程中的 History 数据渲染为 HTML。
*   **参数**: `--auto-refresh` (在实时优化时开启 5 秒刷新)。

### 4.5 打包发布脚本 (`package_skill.py`)
*   **执行节点**: 整个开发流程的最后一步。
*   **核心功能**: 读取 Skill 目录，将其内容打包为 `.skill` 格式（通常是 zip 压缩包）。

### 4.6 快速验证脚本 (`quick_validate.py`)
*   **执行节点**: 手动触发。
*   **核心功能**: 单次检查 `SKILL.md` 的 YAML 格式、文件是否存在等静态验证。

### 4.7 聚合基准脚本 (`aggregate_benchmark.py`)
*   **执行节点**: 在子代理完成多轮测试后。
*   **核心功能**: 遍历 `workspace/iteration-N/` 下的所有 `grading.json` 和 `timing.json`，计算通过率、Token 消耗和耗时的均值与方差。

### 4.8 辅助工具类 (`utils.py`)
*   **核心功能**: 提供 `parse_skill_md` (解析 YAML Frontmatter) 等基础函数，被几乎所有脚本引用。

---

## 6. 代码适配路线图

为了让 `skill-creator` 在当前环境中稳定运行，需要针对 `nova_cli` 和路径管理进行以下改造：

### 6.1 `run_eval.py` 改造需求
*   **根目录搜索逻辑 (`find_project_root`)**:
    *   **现状**: 搜索 `.claude/` 目录。
    *   **待适配**: 应优先寻找 `.nova/` 或 `.gemini/` 目录，以匹配当前项目的管理结构。
*   **命令行构造 (`cmd` 列表)**:
    *   **现状**: 调用 `claude -p`。
    *   **待适配**: 需要切换为调用 `nova_cli` 或具体的入口脚本。应支持输出格式适配（如从 `stream-json` 适配到当前网关协议）。
*   **结果解析 (Nova CLI 兼容性)**:
    *   **现状**: 解析特定于原项目的 `stream_event`。
    *   **待适配**: 必须重新编写事件解析逻辑。需要监听 `tool_use` 事件并判断触发的工具名（如 `Skill` 或 `Read`）以及对应参数。
    *   **注意**: 需对接 `nova_cli` 的底层 JSON 输出结构。

### 6.2 `run_loop.py` 参数分析
结合 `target/request` 及源码，推导关键调用参数如下：

*   **内部调用 `run_eval.py` (L89-L99)**:
    *   `eval_set`: `all_queries` (来自 `evals.json`)
    *   `skill_name`: 动态解析自 `SKILL.md`
    *   `timeout`: 默认 30 秒
    *   `model`: 必须显式指定（如 `gemini-3-flash`）
*   **主入口参数 (L244-L259)**:
    *   `--eval-set`: 必须指定测试集的完整路径。
    *   `--skill-path`: 目标技能文件夹路径。
    *   `--max-iterations`: 建议设置为 5 以平衡时长与质量。
    *   `--holdout`: 建议 0.4。

### 6.3 `improve_description.py` 改造需求
*   **优化指令发送**:
    *   **现状**: 使用 `claude -p --output-format text` 进行同步阻塞调用。
    *   **待适配**: 需适配 `nova_cli` 的调用方式。考虑到该调用通常涉及长响应，需确保 `timeout` 设置足够长（建议 300 秒以上，解决之前的报错问题）。
*   **执行参数推导**:
    *   `--eval-results`: 上一轮 `run_eval.py` 生成的 JSON 输出。
    *   `--skill-path`: 技能目录路径。
    *   `--history`: 之前数轮的改进历史，用于避免模型重复同样的错误路径。

---

## 7. 结论：重点突破项
1.  **沙箱隔离**：在 subagent 启动前，必须通过 `copy_file` 命令预装载技能内容。
2.  **CLI 适配**：将所有硬编码的 `claude` 命令调用改为 `nova_cli` 封装，并重写事件流解析逻辑。

1.  **沙箱隔离导致的 Skill 加载失败**: Subagent 在没有显式文件内容注入的情况下，无法通过相对路径跨沙箱读取 Skill。
2.  **修复策略**:
    *   **注入内容而非路径**: 在 `system_prompt_patch` 中直接传入 `SKILL.md` 的内容原文。
    *   **预先同步**: 在主代理启动 `spawn_subagent` 之前，先利用 `bash` 将 Skill 目录递归复制到目标 `workspace` 下。
