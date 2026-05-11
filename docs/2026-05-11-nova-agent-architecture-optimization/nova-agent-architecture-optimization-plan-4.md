# Plan 4：模块拆分与回归测试补齐

## 前置依赖
- Plan 1
- Plan 2
- Plan 3

## 本次目标
- 在前面三个 Plan 明确完职责边界后，对 `nova-agent` 中最核心的超长文件做按职责拆分。
- 同步补齐正常路径、边界条件、错误路径测试，确保拆分不掺杂行为回归。
- 将 `mod.rs` 收敛为导出、装配和少量稳定入口，减少单文件承载过多流程细节。

## 涉及文件
- `crates/nova-agent/src/conversation/service/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/skill/registry.rs`
- 相关 `mod.rs`、新增子模块文件与对应测试文件

## 现状依据
- `conversation/service/mod.rs` 同时包含启动加载、缓存协同、会话创建/复制、消息追加、标题生成、持久化、并发控制和大量测试。
- `agent/runtime.rs` 同时承担 turn loop、provider streaming、tool call 执行、prompt diagnostics、tool result compaction、side channel 和部分测试。
- `prompt/mod.rs` 与 `prompt/context.rs` 共同承担 prompt 组装、上下文读取、模板拼接、环境快照等职责，运行时 I/O 边界不够清晰。
- `skill/registry.rs` 同时包含 skill/package 数据结构、目录发现、文件解析、策略过滤、查询和测试。

## 详细设计
### 1. 拆分原则
- 拆分必须以职责收敛为核心，不做机械按行数切割。
- 每次拆分优先保持：
  - 对外导出接口名称不变。
  - 业务语义不变。
  - trait 实现位置可追踪。
  - 单次 patch 聚焦于一个模块簇，避免跨多个热点文件大面积挪动。
- 推荐方式是“先抽私有辅助模块，再决定是否提升为独立公开模块”。
- 拆分 patch 中避免混入行为优化；若发现必须改行为，应回到对应 Plan 更新设计并单独实施。

### 2. `conversation/service` 拆分建议
- 当前文件同时包含：初始化、缓存协同、会话创建/复制、消息追加、标题生成、持久化、并发控制、测试。
- 推荐拆分为：
  - `conversation/service/mod.rs`：保留 `SessionService` 类型定义、构造函数、对外 re-export 和少量稳定入口。
  - `conversation/service/load.rs`：索引加载、冷加载、缓存协调、并发去重。
  - `conversation/service/write.rs`：创建、复制、追加消息、更新 title/control/project_dir。
  - `conversation/service/query.rs`：列表、查找最新会话、存在性检查、只读元数据查询。
  - `conversation/service/title.rs`：自动标题生成与策略。
  - `conversation/service/persist.rs`：全量重建型持久化、增量写入协调。
  - `conversation/service/tests.rs` 或 `conversation/service/tests/*.rs`：将大块内联测试迁移出去。
- 若 Rust 模块可见性导致拆分困难，优先使用 `pub(super)`，不要为了省事扩大为 `pub`。

### 3. `agent/runtime` 拆分建议
- 当前文件同时承担：turn loop、provider streaming、tool call 执行、prompt diagnostics、tool result compaction、部分测试。
- 推荐拆分为：
  - `agent/runtime/mod.rs`：保留 `AgentRuntime` 公开入口、构造函数和 re-export。
  - `agent/runtime/turn_loop.rs`：轮次主循环、loop guard 协调、turn result 汇总。
  - `agent/runtime/tool_exec.rs`：工具调用调度、timeout、tool result 转换、side channel 输出。
  - `agent/runtime/stream.rs`：provider streaming chunk 处理、增量消息组装。
  - `agent/runtime/diagnostics.rs`：prompt diagnostics、大输出压缩、debug 信息。
  - `agent/runtime/context.rs`：本轮 `ToolContext` / prompt context 构造辅助。
  - `agent/runtime/tests.rs` 或 `agent/runtime/tests/*.rs`：聚合测试。
- 拆分时应避免循环引用：`mod.rs` 定义核心类型，子模块通过 `impl<C: LlmClient> AgentRuntime<C>` 扩展方法。

### 4. `prompt` 拆分建议
- `prompt/mod.rs` 推荐按以下职责拆分：
  - `prompt/mod.rs`：公共类型导出和 builder 入口。
  - `prompt/builder.rs`：prompt 组装主流程。
  - `prompt/context.rs`：项目上下文和开发提示词加载；在 Plan 2 后应是 async-only runtime path。
  - `prompt/environment.rs`：`EnvironmentSnapshot` 与环境信息格式化。
  - `prompt/templates.rs`：静态模板和文本片段。
  - `prompt/trimmer.rs`：历史裁剪、token/长度相关策略。
  - `prompt/diagnostics.rs`：prompt diagnostics 输出。
- 如果某些类型已被外部模块直接引用，拆分后通过 `pub use` 保持原引用路径稳定。

### 5. `skill/registry` 拆分建议
- `skill/registry.rs` 推荐拆成：
  - `skill/registry/mod.rs`：`SkillRegistry` 类型、公共入口和 re-export。
  - `skill/registry/types.rs`：`Skill`、`SkillPackage`、metadata、policy 相关数据结构。
  - `skill/registry/discovery.rs`：目录遍历、包发现。
  - `skill/registry/parser.rs`：`SKILL.md`、`skill.toml` 解析，保持纯函数可测。
  - `skill/registry/filter.rs`：capability/tool policy、启用条件、查询过滤。
  - `skill/registry/tests.rs` 或 `skill/registry/tests/*.rs`：测试。
- Plan 2 决定 skill discovery 是 startup-only 同步还是 runtime async 后，本 Plan 只做文件拆分，不再重新讨论 I/O 策略。

### 6. 测试补齐策略
- 每次拆分至少保留或新增三类验证：
  - 正常路径：核心主流程继续工作。
  - 边界条件：空输入、缺失配置、懒加载首次访问、重复调用等。
  - 错误路径：I/O 失败、序列化失败、网络超时、工具执行失败。
- 若某个大文件当前测试过于内联，应借拆分机会把测试移到更稳定的模块边界上，而不是继续堆在 `mod.rs` 内。
- 测试命名应表达行为，而不是表达内部实现文件名，避免未来再次拆分时大量改名。

### 7. 拆分顺序
1. `conversation/service`：依赖 Plan 1 后的新加载/写入边界，优先拆出 query/load/write。
2. `agent/runtime`：依赖 Plan 3 后的工具状态边界，拆出 turn_loop/tool_exec/context。
3. `prompt`：依赖 Plan 2 后的 async I/O 边界，拆出 builder/context/environment/templates。
4. `skill/registry`：依赖 Plan 2 后的 discovery 生命周期决策，拆出 parser/discovery/filter/types。

### 8. Review 与回归控制
- 每个模块簇拆分应作为独立 patch，避免一次性迁移所有大文件。
- 每个 patch 应包含：
  - 文件移动/拆分说明。
  - 对外 API 不变说明。
  - 新增或迁移的测试说明。
- 对纯移动代码，应尽量减少格式外改动；对必须修改的可见性，优先使用最窄作用域。

## 测试案例
### 正常路径
- 拆分后现有会话、prompt、工具执行、技能注册相关测试全部保持通过。
- 外部模块仍可通过原有路径引用必要类型和函数。
- `AgentRuntime::run_turn`、会话创建/追加消息、prompt 构建、skill 查询主流程行为不变。

### 边界条件
- 子模块重导出后，外部调用路径与 trait 实现不发生名称或可见性回归。
- `pub(super)` / `pub(crate)` 设置不导致测试只能通过扩大可见性实现。
- 空 skill 目录、空 prompt context、空会话列表等边界测试仍通过。

### 异常场景
- 文件拆分过程中若遗漏导出、循环依赖或模块私有性设置错误，编译检查能快速暴露问题。
- 迁移测试时若丢失错误路径覆盖，新增测试应能覆盖 repository 错误、I/O 错误、工具执行错误等场景。
- 拆分后 clippy 不出现未使用导出、重复 import、过宽可见性等 warning。

## 验收标准
- 四个目标模块至少完成第一轮职责拆分，`mod.rs` 不再承载大量无关实现细节。
- 拆分过程中不改变产品行为和协议语义。
- 测试覆盖包含正常路径、边界条件、异常场景，并与新模块边界匹配。
- 通过 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace`。
