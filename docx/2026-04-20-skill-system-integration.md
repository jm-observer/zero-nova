# 2026-04-20-skill-system-integration

| 章节 | 说明 |
|-----------|------|
| 时间 | 2026-04-20 |
| 项目现状 | 目前 zero-nova 仅支持内置 Tool 和 MCP 协议，不支持 .nova/skills 目录下的 Markdown 定义的技能包。 |
| 本次目标 | 适配 `skill-creator` 以及其他遵循 Nova 规范的本地技能包，使其指令能被集成到 Agent 的系统提示词中。 |
| 详细设计 | 1. 引入 `Skill` 模块，用于扫描和解析 `.nova/skills` 目录。<br>2. 解析 `SKILL.md` 中的 YAML Frontmatter（获取名称和描述）及 Body（获取指令）。<br>3. 在 Gateway 启动时加载所有技能。<br>4. 将技能指令以特定的格式追加到 Agent 的 System Prompt 中。<br>5. 确保 `BashTool` 已注册，以便技能能执行其附带的脚本。 |
| 测试案例 | 1. 验证 `.nova/skills` 目录被正确扫描。<br>2. 验证 `skill-creator` 的描述和指令被注入到 API 请求中（日志观察）。<br>3. 模拟 Agent 调用 `bash` 执行 skill-creator 脚本。 |
| 风险与待定项 | 目前仅支持加载指令，暂不支持动态加载 Skill 元数据到 Tool 定义中（未来可扩展）。 |
