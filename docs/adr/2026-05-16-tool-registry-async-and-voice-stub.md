# 2026-05-16 Tool Registry Async And Voice Stub

## 背景

`nova-agent` 中存在两处需要立即收敛的问题：

- `ToolRegistry` 同时暴露同步和异步公共接口，同步路径依赖 `tokio::sync` 锁上的自旋读取，维护成本高且不适合运行时主路径。
- `AgentApplicationImpl` 已将语音接口暴露给网关，但实际实现仍为 `todo!()`，收到真实请求会 panic。

## 决策

- 删除 `ToolRegistry` 的同步公共注册与读取接口，只保留异步公共方法。
- 将所有调用方统一迁移到异步接口，包括 loader、built-in tools 装配、CLI 和测试。
- 将应用层 `voice_transcribe` / `voice_tts` 的未实现路径改为显式返回 `anyhow::bail!("voice not implemented")`。

## 影响范围

- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/builtin/**`
- `crates/nova-agent-loader/src/bootstrap.rs`
- `crates/nova-cli/src/main.rs`
- `crates/nova-agent/src/app/application.rs`

## 取舍

- 未选择继续保留同步兼容层，因为那会让调用方继续依赖双轨 API，并保留自旋辅助逻辑。
- 未在本次接入真实 `VoiceService`，因为本次目标是先消除 panic 风险，而不是补完语音功能。

## 文档同步

- 新增 `docs/design/system-overview.md`
- 新增 `docs/design/nova-agent-engine-boundaries.md`

## 关联项

- `docs/2026-05-16-tool-registry-async-and-voice-stub/tool-registry-async-and-voice-stub.md`
- `docs/2026-05-16-tool-registry-async-and-voice-stub/tool-registry-async-and-voice-stub-plan-1.md`
- `docs/2026-05-16-tool-registry-async-and-voice-stub/tool-registry-async-and-voice-stub-plan-2.md`
