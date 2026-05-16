# Tool Registry Async And Voice Stub

## 时间

- 创建日期：2026-05-16
- 最后更新：2026-05-16

## 项目现状

`crates/nova-agent` 当前有两处需要立即收敛的问题：

- `ToolRegistry` 同时暴露同步和异步公共接口。同步接口内部通过 `try_lock` + `std::thread::yield_now()` 自旋读取 `tokio::sync` 锁，调用点已经扩散到 `nova-agent`、`nova-cli` 和测试中，增加了维护复杂度，也不适合作为运行时内的主路径。
- `AgentApplicationImpl` 已经对外暴露 `voice_transcribe` / `voice_tts` 能力，但实现仍保留 `todo!()`。网关收到语音请求时会真正调用这些接口，当前行为会直接 panic。

## 整体目标

本次改动只做两件事，并保持范围收敛：

1. 将 `ToolRegistry` 的公共读取与注册接口统一为异步形式，删除同步自旋路径。
2. 将应用层语音未实现路径改为显式错误返回 `anyhow::bail!("voice not implemented")`，避免运行时 panic。

## Plan 拆分

| Plan | 描述 | 依赖关系 | 执行顺序 | 状态 |
| --- | --- | --- | --- | --- |
| Plan 1 | 统一 `ToolRegistry` 为异步公共接口并调整全部调用面 | 无 | 1 | 已完成 |
| Plan 2 | 将应用层语音未实现路径改为显式错误返回并补测试 | Plan 1 | 2 | 已完成 |

## 风险与待定项

- `ToolRegistry` 的同步接口删除后，会影响 `nova-cli`、prompt 构建和大量单元测试；需要逐一收口，不能只改 crate 内主路径。
- 语音接口本次只做防 panic 兜底，不接入真实 `VoiceService`，因此需要保持错误消息稳定、直接。
- 本次会影响长期设计资产，需要补充最小的设计基线和设计影响记录，说明 `ToolRegistry` 不再提供同步公共 API，以及语音能力在未接线前的失败语义。

## 非目标

- 不在本次任务中接入真实语音 STT/TTS 实现。
- 不在本次任务中重构 `ToolRegistry` 的内部数据结构或 deferred tool 语义。
- 不新增依赖。

## 验收标准

- `ToolRegistry` 不再暴露同步公共注册/读取接口。
- 所有 `ToolRegistry` 调用点均改为异步使用并编译通过。
- `voice_transcribe` / `voice_tts` 不再 panic，而是返回明确错误。
- 覆盖新增或修改后的正常路径、错误路径测试。
- `cargo clippy --workspace -- -D warnings` 通过。
- `cargo fmt --check --all` 通过。
- `cargo test --workspace` 通过。
