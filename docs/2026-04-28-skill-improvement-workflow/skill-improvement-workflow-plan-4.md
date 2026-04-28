# Plan 4: 报告审阅、恢复能力与测试闭环

## 前置依赖
- Plan 1
- Plan 2
- Plan 3

## 本次目标
- 补齐面向用户的审阅报告、恢复机制和最终写回流程。
- 为新增工作流补充正常、边界、异常场景测试，确保该功能可持续维护。
- 形成可交付闭环：从“开始改进”到“导出最佳 skill / 写回原 skill / 复现本次结果”。

## 涉及文件
- `.nova/skills/skill-creator/scripts/generate_report.py`
- `.nova/skills/skill-creator/eval-viewer/generate_review.py`
- `.nova/skills/skill-creator/eval-viewer/viewer.html`
- 新增 `.nova/skills/skill-creator/scripts/finalize_improvement.py`
- 新增 `.nova/skills/skill-creator/scripts/resume_improvement.py`
- `crates/nova-cli/src/main.rs`
- `crates/nova-agent/src/skill.rs`

## 详细设计
- 报告输出分为两层：
  - **迭代内报告**：每轮 `report.html` 展示候选修改点、得分对比、失败样本、代表性输出。
  - **最终报告**：`final-report.html` 汇总最佳轮次、整体提升幅度、仍未解决的问题、是否建议写回。
- `generate_report.py` 扩展输入结构，除 eval 结果外还读取：
  - 会话元数据
  - 候选 diff 摘要
  - 失败分类结果
  - 人工审阅结论（如果有）
- 新增 `resume_improvement.py`：
  - 从 `improvement-session.json` 读取最近状态。
  - 校验工作区完整性。
  - 若上轮只执行到一半，决定从“重新评测本轮”还是“跳到下一轮”恢复。
- 新增 `finalize_improvement.py`：
  - 将 `best-skill/` 与原 skill 做最终校验。
  - 默认生成 diff 和报告，不直接覆盖原目录。
  - 用户确认后再写回；写回前保留 `original-backup/`。
- CLI 层若增加薄封装，建议只做用户体验补足：
  - 例如 `nova-cli skill improve --path <skill>` 负责启动/恢复会话。
  - 真正的评测与优化逻辑仍放在 `skill-creator/scripts/`，避免 Rust 入口承担过多业务细节。
- 测试策略分三层：
  - 脚本级：JSON schema、路径解析、状态恢复、得分聚合。
  - 工作流级：从初始化到最终报告的端到端 smoke test。
  - 宿主集成级：验证 `--include-skill` 加载、skill registry 可见性、事件输出是否符合预期。

## 测试案例
- 正常路径：完整执行 2~3 轮改进后，生成最终报告并可导出最佳 skill。
- 恢复路径：在第 2 轮中断后，恢复脚本能够识别中间状态并继续完成。
- 写回路径：用户确认后，最佳版本正确写回，同时保留原始备份。
- 错误路径：工作区文件缺失、会话 JSON 损坏、最佳版本目录不存在时，给出明确错误并拒绝写回。
- 集成路径：通过 `nova-cli --include-skill <candidate>` 运行 smoke test，确认最终导出版本能被宿主正常发现和加载。
