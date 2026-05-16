# Plan 2: 语音未实现路径改为显式错误

## 前置依赖

Plan 1

## 任务目标

将应用层语音未实现路径从 `todo!()` 改为显式错误返回，保证网关收到语音请求时不会 panic。完成后：

- `voice_transcribe` 返回 `anyhow::bail!("voice not implemented")`
- `voice_tts` 返回 `anyhow::bail!("voice not implemented")`
- 增加单元测试验证错误消息

## 执行范围

| 类别 | 路径 | 说明 |
| --- | --- | --- |
| 必须修改 | `crates/nova-agent/src/app/application.rs` | 删除 `todo!()`，改为显式错误 |
| 允许修改 | `crates/nova-gateway-core/src/handlers/voice.rs` | 仅在测试或类型适配需要时修改 |
| 禁止修改 | `crates/nova-agent/src/app/voice_service.rs` | 不在本 Plan 中接入真实语音实现 |

## Agent 执行步骤

1. 在 `AgentApplicationImpl::voice_transcribe` 中删除 `todo!()`，改为 `anyhow::bail!("voice not implemented")`。
2. 在 `AgentApplicationImpl::voice_tts` 中删除 `todo!()`，改为 `anyhow::bail!("voice not implemented")`。
3. 在 `application.rs` 中新增单元测试，直接断言两条路径返回错误，且错误消息包含 `voice not implemented`。
4. 保持 `voice_capabilities` 当前行为不变，不在本 Plan 中伪装为“可用但失败”的真实 provider 接入。

## 行为规则

| 输入 / 场景 | 处理路径 | 期望结果 |
| --- | --- | --- |
| 网关调用 `voice_transcribe` | 应用层显式返回错误 | 不 panic，错误消息包含 `voice not implemented` |
| 网关调用 `voice_tts` | 应用层显式返回错误 | 不 panic，错误消息包含 `voice not implemented` |
| 调用 `voice_capabilities` | 保持现有实现 | 行为不变 |

## 禁止事项

- 不要接入真实 `VoiceService`。
- 不要修改语音协议字段。
- 不要把未实现错误改为静默成功或空 payload。

## 测试要求

| 测试文件 | 测试名称 | 输入 | 期望断言 |
| --- | --- | --- | --- |
| `crates/nova-agent/src/app/application.rs` | `voice_transcribe_returns_not_implemented_error` | 任意请求对象 | 错误包含 `voice not implemented` |
| `crates/nova-agent/src/app/application.rs` | `voice_tts_returns_not_implemented_error` | 任意请求对象 | 错误包含 `voice not implemented` |

必须执行的验证命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `voice_transcribe` 不再使用 `todo!()`
- [ ] `voice_tts` 不再使用 `todo!()`
- [ ] 语音未实现路径有单元测试覆盖
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
