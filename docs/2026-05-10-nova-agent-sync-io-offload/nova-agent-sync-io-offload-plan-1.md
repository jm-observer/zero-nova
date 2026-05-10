# Plan 1: skill.rs 技能扫描链路异步化

## 前置依赖
- 无

## 本次目标
- 将 `crates/nova-agent/src/skill.rs` 内技能目录扫描和技能文件读取从同步 I/O 改为异步可等待实现。
- 保持技能发现行为不变：递归策略、`skill.toml` 优先、`SKILL.md` 回退、日志级别与容错语义保持一致。

## 涉及文件
- `crates/nova-agent/src/skill.rs`
- `crates/nova-agent/src/app/bootstrap.rs`（调用入口改造）
- `crates/nova-agent/src/skill.rs` 对应测试模块（如存在）

## 详细设计
1. 接口分层
- 保留原 `SkillRegistry` 结构体不变，新增异步入口：
  - `pub async fn load_from_dir_async<P: AsRef<Path>>(&mut self, dir: P) -> Result<()>`
  - `async fn scan_dir_recursive_async(dir: &Path, registry: &mut SkillRegistry) -> Result<()>`
  - `async fn parse_skill_file_async(&self, path: &Path) -> Result<Skill>`
  - `async fn parse_skill_toml_async(&self, path: &Path) -> Result<SkillPackage>`
- 原同步方法处理策略：
  - 短期保留，但在文档与注释标记为“仅限非 async 上下文”。
  - `bootstrap` 与其他 async 主路径全部切换到 async 版本。

2. I/O 实现策略
- 目录遍历：
  - 优先使用 `tokio::fs::read_dir` + `next_entry().await`。
  - 递归过程中仅传递 `PathBuf`，避免借用跨 `await` 生命周期复杂化。
- 文件读取：
  - 使用 `tokio::fs::read_to_string` 读取 `SKILL.md` / `skill.toml`。
- 解析逻辑：
  - 继续复用现有字符串分割、frontmatter 提取、toml 字段映射逻辑，避免行为漂移。

3. 行为一致性约束
- `load_single_skill` 的语义保持：
  - 有 `skill.toml` 时先解析；失败记录 `warn` 后回退 `SKILL.md`。
  - 无可用文件时 `Ok(())`。
- 不新增重试、不引入缓存、不调整排序，避免“优化夹带行为变更”。

4. 调用侧调整
- 在 `build_application` 中改为：
  - `skill_registry.load_from_dir_async(&skill_dir).await`
- 保持现有 `if let Err(err)` 的降级策略与日志输出。

## 测试案例
1. 正常路径
- 用临时目录构造多层技能目录（含 `skill.toml` 与 `SKILL.md`），验证异步加载后 `skills/packages` 数量、关键字段与同步版本一致。

2. 边界条件
- 空目录、目录不存在、仅含空白 `SKILL.md`、无 frontmatter `SKILL.md`（compat mode）均不 panic，返回符合现有语义。

3. 异常路径
- `skill.toml` 非法内容：应 `warn` 并回退 `SKILL.md`。
- 文件读取权限错误：返回 `Err` 或按既有降级策略记录日志（与当前行为保持一致）。

4. 回归验证
- 对比改造前后同一技能目录加载结果（可通过测试断言关键字段一致）：
  - `id/slug/display_name/tool_policy/compat_mode` 不变化。
