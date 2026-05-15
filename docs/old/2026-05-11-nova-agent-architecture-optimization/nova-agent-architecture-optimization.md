# nova-agent 架构与性能优化设计总览

## 时间
- 创建时间：2026-05-11
- 最后更新：2026-05-11

## 项目现状
- `crates/nova-agent` 已具备较完整的分层：`app` 负责应用装配与对外服务，`agent` 负责单轮执行与工具调度，`conversation` 负责会话状态与持久化，`tool` 负责内置工具和注册表，`provider` 负责上游模型适配，`prompt` 负责提示词构建。
- 当前架构的主要问题不在“缺少层次”，而在部分热点路径仍存在职责聚合过重、生命周期边界不清和性能策略散落的问题。
- 代码体量上，多个核心文件已明显超过项目约束建议：`crates/nova-agent/src/conversation/service/mod.rs`、`crates/nova-agent/src/agent/runtime.rs`、`crates/nova-agent/src/prompt/mod.rs`、`crates/nova-agent/src/skill/registry.rs` 同时承载流程编排、状态管理、辅助逻辑和测试，后续 review 与回归成本较高。
- 启动链路上，`crates/nova-agent/src/app/bootstrap.rs:52` 通过 `session_service.load_all().await?` 预加载全部会话；`crates/nova-agent/src/conversation/service/mod.rs:76` 当前会把数据库中的所有会话及历史消息加载到内存。该策略在历史数据量较小时实现简单，但启动时间、内存占用和反序列化成本会随历史消息规模增长。
- 运行时 I/O 路径上，`crates/nova-agent/src/prompt/context.rs:404` 与 `crates/nova-agent/src/skill/registry.rs:945` 均存在 `read_to_string_runtime_aware()`，在 Tokio runtime 内通过 `block_in_place + block_on` 或 `block_in_place` 兼容同步读路径。该方案可避免最直接的 async worker 阻塞，但仍保留同步/异步双轨接口，增加心智负担和误用概率。
- 工具与 HTTP 访问路径上，`crates/nova-agent/src/provider/anthropic.rs:24`、`crates/nova-agent/src/provider/openai_compat/mod.rs:44`、`crates/nova-agent/src/tool/builtin/web_fetch.rs:19`、`crates/nova-agent/src/tool/builtin/web_search/mod.rs:27`、`crates/nova-agent/src/provider/health.rs:64`、`crates/nova-agent/src/voice/openai_compat.rs:18` 分别创建 `reqwest::Client` 或 builder，连接池、超时、代理、User-Agent、追踪策略难以统一。
- 状态与并发路径上，任务工具共享 `Arc<tokio::sync::Mutex<TaskStore>>`，见 `crates/nova-agent/src/agent/runtime.rs:46`、`crates/nova-agent/src/tool/registry.rs:28`、`crates/nova-agent/src/tool/builtin/task.rs:248`。会话服务采用“内存缓存 + SQLite 持久化”双写模式，当前能满足功能，但在并发和长会话场景下存在锁粒度偏粗、写放大和热路径语义不够明确的问题。
- 读缓存路径已经出现较好的局部优化：`crates/nova-agent/src/agent/runtime.rs:508` 每轮创建 `TurnReadState`，`crates/nova-agent/src/tool/read_cache.rs:17` 维护 turn-scoped 文件读取状态。后续优化应保留该生命周期边界，避免把单轮缓存误提升为 session/global 状态。

## 整体目标
- 在不改变现有产品语义、对外协议和用户可见行为的前提下，对 `nova-agent` 做一次聚焦于可维护性、性能与运行时安全的内部优化。
- 将本次优化收敛为一组小而可验证的实施 Plan，避免将“模块拆分”“性能优化”“行为修正”混杂到单次改动中。
- 统一以下关键约束：
  - 运行时热路径优先使用纯异步 I/O，不依赖同步桥接作为常规方案。
  - 启动阶段与运行阶段职责边界清晰，startup-only 路径不外溢到运行时。
  - 高频读取和低频写入场景优先使用快照、惰性加载、增量持久化等方式降低锁竞争与写放大。
  - HTTP 客户端、超时、重定向、User-Agent、上下文头等横切策略优先集中配置和注入，而不是在各模块重复创建。
  - 模块拆分以职责收敛为目标，不改变公开接口语义，不引入额外抽象层级作为目的本身。
  - 每个 Plan 完成后必须执行完整检查流程：`cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace`。

## 非目标
- 不修改前后端协议，不调整 `nova-protocol` 的 schema 定义。
- 不重新设计整个 Agent 执行模型，不引入 actor 框架或事件总线替换现有调用链。
- 不顺带替换 SQLite、SQLx、Tokio 或 provider 协议适配方式。
- 不新增依赖；如确需新增依赖，必须单独提交原因并获得用户确认。
- 不要求一次性消除所有大文件，而是先拆最影响稳定性和性能判断的热点模块。
- 不以基准测试数值作为本阶段硬性目标；本阶段先保证架构边界、复杂度和回归测试可控。

## 关键问题归纳
### 1. 启动全量加载导致扩展性差
- `SessionService::load_all()` 当前是“列出全部 session -> 逐个完整加载 -> 填充内存缓存”的策略。
- 该模式的优点是实现简单、后续读取快，但缺点同样明显：
  - 启动成本与历史数据规模绑定。
  - 很多冷数据在单次运行中不会被访问，却会被提前加载。
  - 内存缓存与数据库之间的双份状态在历史规模增长后更难维护。
- 更合理的边界应是：启动时仅加载必要索引，消息历史按需加载，并通过并发去重确保同一会话不会重复冷加载。

### 2. 同步/异步双轨路径增加误用成本
- `prompt/context.rs` 和 `skill/registry.rs` 目前都维护 runtime-aware 同步读取接口。
- 这类桥接函数会模糊“哪些调用只允许在启动期执行，哪些调用可在运行期热路径执行”的边界。
- 长期看，这类接口会成为技术债入口：后续新调用者容易直接沿用同步接口，继续扩散混合模式。

### 3. 热点状态结构粒度偏粗
- `TaskStore` 通过单个 `Mutex` 保护，适合低并发简单实现，但会让 `TaskCreate` / `TaskUpdate` / `TaskList` 共享同一串行入口。
- 会话持久化当前以整会话视角处理，虽然保证了一致性，但在长会话场景下容易放大 I/O 和序列化成本。
- 这些问题尚未构成功能 bug，但已经成为后续性能优化和复杂功能扩展的约束点。

### 4. 横切策略分散，难统一治理
- HTTP Client 分散创建会带来连接池重复、配置不一致、测试注入困难。
- 一些超时与回退值仍靠局部常量表达，缺少统一归口。
- 个别库内代码仍存在 `unwrap()`、`#[allow(...)]` 或同步 I/O 桥接等不符合项目长期约束的实现，需要在后续 Plan 中一起收敛。

### 5. 超长模块影响评审和演进
- 当前一些文件已同时承载模型、流程编排、辅助函数、测试等多个职责。
- 这会直接带来：
  - 修改一处逻辑时需要理解过大的上下文。
  - 单文件 merge conflict 概率上升。
  - 测试与实现耦合过近，难以按职责演进。

## 推荐结论
- 必做项 1：将会话加载与持久化策略拆为“启动索引加载 + 按需消息冷加载 + 增量更新”，作为首要性能收益点。
- 必做项 2：收敛 prompt / skill / provider 等路径中的同步桥接，只保留明确标记的启动期同步读取能力，运行时统一 async。
- 必做项 3：统一 HTTP Client 注入与超时策略，减少重复客户端创建和横切配置漂移。
- 必做项 4：优先拆分 `conversation/service`、`agent/runtime`、`prompt/mod`、`skill/registry` 四个关键大文件，按职责降低复杂度。
- 建议项：在任务存储等热点状态上预留更细粒度并发模型，但只在不改变外部语义的情况下推进，避免过度设计。

## Plan 拆分
| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 会话加载与持久化路径优化：从全量预加载转向索引加载、按需冷加载与增量持久化 | 无 | 1 | 待开始 |
| Plan 2 | 运行时 I/O 与 HTTP 横切治理：收敛同步桥接、统一异步读路径与共享 `reqwest::Client` 注入 | 可与 Plan 1 并行评审；实施时需避免接口冲突 | 2 | 待开始 |
| Plan 3 | 热点状态与工具执行并发优化：梳理 `TaskStore`、工具上下文和热点锁边界 | Plan 1、Plan 2 | 3 | 待开始 |
| Plan 4 | 模块拆分与回归测试补齐：按职责拆分超长文件，并补正常/边界/异常测试 | Plan 1、Plan 2、Plan 3 | 4 | 待开始 |

执行顺序说明：
- Plan 1 优先，因为它同时影响启动性能、内存占用和后续模块边界定义；若先拆文件再改加载模型，容易反复搬运代码。
- Plan 2 次之，用于明确“运行时只走 async”的架构基线，并把 HTTP 横切策略收口，为后续 provider/tool 演进打底。
- Plan 3 放在中后段，基于前两项梳理过的边界再细化锁与状态结构，避免过早引入不必要抽象。
- Plan 4 最后实施，确保拆分动作建立在清晰的职责边界之上，而不是机械切文件。

## 分阶段验收门禁
每个 Plan 完成后必须满足：
- 行为语义：原有对外 API、命令入口、协议 DTO 不发生兼容性破坏。
- 编译检查：`cargo clippy --workspace -- -D warnings` 通过。
- 格式检查：`cargo fmt --all` 已执行且无格式漂移。
- 测试检查：`cargo test --workspace` 通过。
- 文档同步：如实际实施偏离本设计，需更新对应 Plan 文档而不是只改代码。

## 风险与待定项
- 风险 1：Plan 1 会触碰会话缓存、SQLite 仓储和应用启动流程，若边界定义不清，容易出现“缓存命中语义改变”或“消息读取时机改变”的回归。
- 风险 2：Plan 2 会影响 prompt 构建、skill discovery 和 provider/tool 访问路径，若同步兼容路径一次性删除过多，可能波及测试辅助代码。
- 风险 3：Plan 3 若过度追求细粒度锁，可能引入更复杂的一致性问题；因此必须坚持“先收敛职责，再调整并发结构”。
- 风险 4：Plan 4 涉及大文件拆分，若与行为修改混在一起，review 和回归成本会急剧上升。
- 待定项 1：`TaskStore` 最终采用 `RwLock`、service/repository 化还是保留 `Mutex` 并缩短持锁区，需要在 Plan 3 中结合现有调用模式定稿。
- 待定项 2：共享 HTTP Client 的注入层级是放在 `app/bootstrap` 统一创建，还是新增 `network` 装配模块，需要在 Plan 2 中结合测试替身方式确定。
- 待定项 3：`voice/openai_compat.rs` 是否纳入本轮共享 HTTP Client 治理。若 voice 模块与 agent provider 生命周期不同，可作为 Plan 2 的延伸项单独处理，但文档和代码中需明确边界。

## 最终验收标准
- 启动阶段不再因历史会话数量增长而线性放大全量消息加载成本。
- 运行时热路径不再依赖 `block_in_place + block_on` 这类同步桥接作为常规实现。
- provider、Web 工具和可纳入范围的 voice HTTP 路径可复用统一配置的异步 HTTP Client，超时与连接策略可集中治理。
- `TaskStore`、会话缓存和工具上下文的锁边界更清晰，读多写少场景下竞争降低且无语义回归。
- 大文件拆分后，`mod.rs` 主要承担导出与装配职责，核心流程、状态、测试按职责归位。
- 至少完成一轮针对正常路径、边界条件、错误路径的测试补齐，并通过完整检查流程。
