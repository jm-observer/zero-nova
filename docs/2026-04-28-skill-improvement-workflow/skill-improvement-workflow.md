# skill-improvement-workflow 设计总览

## 时间
- 创建时间：2026-04-28
- 最后更新：2026-04-28

## 项目现状
- 当前 `.nova/skills/skill-creator/SKILL.md` 已覆盖“创建 skill、修改已有 skill、运行 eval、优化 description”这几类工作，但整体仍偏“人工编排”。
- `.nova/skills/skill-creator/scripts/` 下已经存在 `quick_validate.py`、`run_eval.py`、`improve_description.py`、`run_loop.py` 等脚本，说明仓库已经具备“校验、评测、描述优化、循环优化”的基础能力。
- 现有能力的主要缺口不在“有没有脚本”，而在“是否形成了面向已有 skill 的一条稳定工作流”：
  - 缺少显式的“改进现有 skill”模式入口，用户需要自己拼接步骤。
  - 缺少目标 skill 的统一加载/校验/快照流程，容易直接在原目录上反复修改。
  - `run_loop.py` 当前聚焦 description 触发优化，不等价于“对整个 skill 反复测试、比较、回滚、收敛”。
  - 评测产物、迭代历史、失败原因、最佳版本选择策略还没有形成统一约定。
- `crates/nova-agent/src/skill.rs` 与 `crates/nova-cli/src/main.rs` 已具备 skill 扫描、解析、`--include-skill` 动态加载与事件输出能力，说明“加载目标 skill 做真实运行”已有宿主基础，不需要从零设计执行环境。

## 整体目标
- 为 `skill-creator` 新增一套**面向已有 skill 的改进工作流**，支持：
  - 加载目标 skill。
  - 校验并快照目标 skill。
  - 基于真实 eval 反复测试。
  - 生成候选修改并比较新旧效果。
  - 保留每轮产物、得分、失败原因和最佳版本。
  - 在收敛后将最佳结果安全写回目标 skill。
- 将现有“description 优化”扩展为“**skill 全量改进 loop**”：既能优化前置触发描述，也能优化 `SKILL.md` 中的正文指令、示例与评测集。
- 保持实现尽量复用现有资产：优先复用 `.nova/skills/skill-creator/scripts/` 与 `nova-cli --include-skill`，避免引入新的 Rust 依赖或重写整套评测基础设施。
- 输出面向用户可审阅的过程产物，使“为什么本轮更好/更差”可追踪、可复现、可回滚。

## 核心设计原则
- **真实执行优先**：所有“测试”“优化”“比较”都以真实加载 skill 后的运行结果为准，不允许只在对话里假设效果。
- **原目录保护**：默认只在工作区副本中进行迭代，最终用户确认后再写回原 skill 目录。
- **评测先行**：没有可解释的 eval 集和评分结果，就不进入自动优化循环。
- **最小侵入**：优先增强 `skill-creator` skill 自身及其辅助脚本，必要时再增加 `nova-cli` 的薄封装入口。
- **可恢复**：任一迭代中断后，都应能从会话元数据恢复并继续执行。

## 建议工作流
1. 用户提供目标 skill 路径或 skill id。
2. 系统解析并校验目标 skill，创建改进会话目录与只读快照。
3. 选择或生成 eval 集，补齐触发类与行为类测试。
4. 执行基线评测：原 skill、无 skill、候选 skill 三路可比较。
5. 生成候选修改，写入临时副本。
6. 对候选版本运行评测并与基线比较。
7. 记录本轮结果；若变好则晋升为当前最佳，否则回滚并总结失败模式。
8. 达到收敛条件后，输出最终报告并由用户决定是否写回原 skill。

## 产物布局建议
- 以目标 skill 同级目录创建 `<skill-name>-workspace/`。
- 在工作区内统一使用以下结构：

```text
<skill-name>-workspace/
├── improvement-session.json
├── target-skill-snapshot/
├── evals/
│   ├── trigger-evals.json
│   └── behavior-evals.json
├── iterations/
│   ├── iteration-001/
│   │   ├── candidate-skill/
│   │   ├── eval-results.json
│   │   ├── score-summary.json
│   │   ├── report.html
│   │   └── notes.md
│   └── iteration-002/
├── best-skill/
└── final-report.html
```

## Plan 拆分
| Plan | 简要描述 | 依赖 | 执行顺序 | 完成状态 |
|---|---|---|---|---|
| Plan 1 | 明确“改进已有 skill”入口、会话模型与工作区约定 | 无 | 1 | 待开始 |
| Plan 2 | 打通目标 skill 的加载、校验、基线评测与迭代执行链路 | Plan 1 | 2 | 待开始 |
| Plan 3 | 扩展自动优化循环，支持候选生成、比较、回滚与收敛 | Plan 1, Plan 2 | 3 | 待开始 |
| Plan 4 | 补齐报告、恢复能力与测试，形成可交付工作流 | Plan 1, Plan 2, Plan 3 | 4 | 待开始 |

## 范围外事项
- 不在本次设计中引入新的远程评测服务或数据库。
- 不将 skill 优化扩展为通用“任意提示词工程平台”。
- 不自动改写目标 skill 目录下的任意代码文件；默认只修改 `SKILL.md`、评测集与本 skill 所管理的辅助文件。
- 不在本次设计中承诺多模型并行 A/B 排名，只保留未来扩展点。

## 风险与待定项
- 风险：自动优化可能过拟合小规模 eval 集，需要保留 holdout 集和人工复审入口。
- 风险：目标 skill 若依赖额外文件、脚本或外部资源，简单复制 `SKILL.md` 不足以形成真实候选版本，需要目录级快照策略。
- 风险：现有 `run_loop.py` 偏 description 优化，扩展到“正文+示例+评测集”后，候选空间会明显增大，必须限制单轮改动范围。
- 风险：如果目标 skill 本身带有不稳定副作用，评测结果可能受外部环境波动影响，需要记录失败类别并支持重复运行。
- 待定：第一阶段是否只增强 `skill-creator` skill 与 Python 脚本，还是同步增加 `nova-cli skill improve` 命令。建议先做前者，后者作为薄封装追加。
