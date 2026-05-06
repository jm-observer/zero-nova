# Plan 3: 自动优化循环、候选生成与回滚收敛

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 将当前偏“description 触发优化”的 `run_loop.py` 扩展为面向整个 skill 的自动改进循环。
- 支持生成候选版本、限制单轮改动范围、比较得分、选择最佳版本、失败回滚与收敛判定。
- 让自动化结果保持可解释，避免出现“分数变高但不知道改了什么”的黑盒体验。

## 涉及文件
- `.nova/skills/skill-creator/scripts/run_loop.py`
- `.nova/skills/skill-creator/scripts/improve_description.py`
- 新增 `.nova/skills/skill-creator/scripts/improve_skill.py`
- 新增 `.nova/skills/skill-creator/scripts/score_iteration.py`
- 新增 `.nova/skills/skill-creator/scripts/apply_candidate.py`
- `.nova/skills/skill-creator/agents/analyzer.md`
- `.nova/skills/skill-creator/agents/comparator.md`
- `.nova/skills/skill-creator/agents/grader.md`

## 详细设计
- 扩展优化对象，不再只盯 `description`：
  - 第一优先级：frontmatter `description`。
  - 第二优先级：`SKILL.md` 正文里的触发条件、步骤顺序、输出约束。
  - 第三优先级：示例 prompts、eval 集和断言描述。
  - 明确禁止优化器直接大范围改写无关辅助脚本，防止问题空间过大。
- 新增 `improve_skill.py`，负责根据上轮失败样本生成候选变更计划：
  - 输入：当前最佳 skill、副本路径、失败样本、评分摘要、历史迭代。
  - 输出：结构化候选补丁计划，例如“仅改 description”“补充一个触发反例”“收紧输出格式要求”。
  - 每轮改动限制在 1~2 个主题，降低过拟合和定位成本。
- 新增 `apply_candidate.py`，将候选补丁应用到 `candidate-skill/`：
  - 仅修改允许的文件集合。
  - 写出 `candidate-diff-summary.json`，记录改动点与原因。
  - 若应用失败，直接终止本轮并回滚。
- `run_loop.py` 调整为会话驱动：
  - 读取 `improvement-session.json`。
  - 逐轮调用 `improve_skill.py -> apply_candidate.py -> run_iteration.py -> score_iteration.py`。
  - 更新 `best_iteration`、`best_score`、`history`。
- 收敛规则建议显式化，避免 magic number 散落：
  - `MAX_ITERATIONS`
  - `MIN_SCORE_DELTA`
  - `MAX_CONSECUTIVE_NO_IMPROVEMENT`
  - `TRIGGER_PASS_THRESHOLD`
  - `BEHAVIOR_PASS_THRESHOLD`
- 回滚策略：
  - 若候选总分低于当前最佳，直接丢弃候选副本，仅保留结果记录。
  - 若总分相同但误触发率更低或关键行为断言通过更多，可允许以次级规则晋升。
  - 若候选引入严重回归（例如原本通过的关键断言大面积失败），可提前结束循环。
- 可解释性增强：
  - `score_iteration.py` 除总分外，还输出失败聚类，如“误触发”“未触发”“格式错误”“遗漏步骤”。
  - 可复用 `analyzer.md` / `comparator.md` 生成“为何该轮更优”的摘要，写入 `notes.md`。

## 测试案例
- 正常路径：候选版本得分提升时，最佳版本正确晋升，历史链路完整记录。
- 回滚路径：候选版本得分下降时，不覆盖当前最佳版本。
- 边界条件：若连续多轮无提升，循环应按阈值正常收敛而非无限运行。
- 错误路径：候选补丁应用失败、生成非法 frontmatter、评测脚本异常时，会话应转为可恢复状态。
- 解释性：每一轮都应产出可读的失败原因摘要，而不是只有一个总分。
