# 2026-04-28 语音对话链路方案设计

## 时间
- 创建日期：2026-04-28
- 最后更新：2026-04-28

## 项目现状

> **注意**：Plan 1 已于 2026-04-28 实施完成（提交 `30c24fb`），以下现状描述已更新至 Plan 1 完成后的状态。

- `deskapp/src/voice.ts` 已具备本地录音（输出 16kHz mono WAV）、VAD 自动停录（两阶段验证防误触）、TTS 播放队列、流式 TTS 分句播放、Barge-In 打断检测、冥想背景音等完整的前端音频能力。
- `deskapp/src/ui/voice-overlay.ts` 已实现完整的语音对话浮层，包括状态文案映射、涟漪动画、实时转录预览、录音计时器、错误提示与重试入口，并通过事件总线与状态机联动。
- `deskapp/src/core/state.ts` 已定义完整的 `VoiceConversationPhase`（idle / requesting_permission / recording / uploading_audio / recognizing / submitting_text / waiting_assistant / speaking / interrupted / error）和 `VoiceConversationState` 接口，包含 `transcript`、`transcriptState`、`canRetry` 等字段。
- `deskapp/src/core/event-bus.ts` 已注册语音事件：`VOICE_MODE_TOGGLE`、`VOICE_STATE_UPDATED`、`VOICE_CAPABILITIES_UPDATED`、`VOICE_CONTROL_START/STOP/RETRY`。
- `deskapp/src/gateway-client.ts` 已实现 `transcribeVoice()`、`synthesizeVoice()`、`getVoiceCapabilities()` 三个 API 方法，采用 Base64 编解码传输音频。
- `crates/nova-protocol/src/voice.rs` 已定义 MVP 所需的 Rust 类型：`VoiceTranscribeRequest/Response`、`VoiceTtsRequest/Response`、`VoiceCapabilitiesRequest/Response`。
- 前端语音交互闭环（录音 → 上传 → 识别 → 文本发送 → 助手回复 → 自动朗读）已在前端侧打通；**后端 STT/TTS 编排层尚未实现**，是当前链路缺失的关键环节。

## 整体目标
- 建立一条可演进的语音对话链路：前端采集语音、通过 Gateway 传输、后端完成识别与文本对话、再将回复以文本和可选音频返回前端。
- 优先复用当前 `voice.ts` 的录音/VAD/TTS/Barge-In 基础能力，避免重做前端音频控制。
- 在协议层明确区分“控制消息”和“音频数据”，使实现既能快速落地首版，也能为后续低延迟流式语音对话预留扩展点。
- 将方案拆成可独立评审与实施的几个 Plan，先交付 MVP，再逐步升级为流式双工语音模式。

## 推荐方案摘要
- 推荐采用“两阶段演进”方案：
  - 第一阶段做“分段语音请求”MVP：前端录音结束后上传一段完整音频，后端完成 STT -> LLM -> TTS，前端显示转写并播放回复。
  - 第二阶段再演进到“流式语音会话”：前端持续分片上传 PCM/Opus，后端增量识别、边生成边播报，并支持更稳定的打断控制。
- 推荐 Gateway 继续作为唯一语音入口：
  - 前端不直接调用 STT/TTS 厂商接口。
  - STT/TTS 模型、鉴权、重试、超时、计费和观测均收敛在后端。
- 推荐协议上采用“WebSocket JSON 控制消息 + 二进制音频帧”的双通道语义：
  - MVP 可先只做 Base64 音频载荷的 JSON 请求，降低首版复杂度。
  - 流式阶段切到真正二进制帧，减少体积与序列化开销。

## Plan 拆分
- Plan 1：前端语音交互与状态机收敛 ✅ **已完成**
  - 描述：整理 `voice.ts`、语音浮层、聊天输入框之间的状态流，补齐从录音到文本发送的前端闭环。
  - 依赖：无。
  - 状态：已于 2026-04-28 实施完成。
- Plan 2：Gateway 语音协议与传输设计
  - 描述：定义 STT/TTS/语音会话相关消息、错误模型、分段上传与流式事件格式。
  - 依赖：Plan 1。
  - 状态：已于 2026-04-28 实施完成。
- Plan 3：后端识别与应答编排
  - 描述：在 Gateway/服务端增加语音编排层，串联 STT、文本会话、TTS 与可观测性。
  - 依赖：Plan 2。
  - 状态：已于 2026-04-28 实施完成。
- Plan 4：测试、灰度与后续演进
  - 描述：补齐协议契约、前端状态机、端到端链路、降级回退和后续流式语音迭代路径。
  - 依赖：Plan 3。
  - 状态：已于 2026-04-28 实施完成。

执行顺序：Plan 1 -> Plan 2 -> Plan 3 -> Plan 4。

## 风险与待定项
- 风险 1：~~浏览器 `MediaRecorder` 默认产出 `webm/opus`，若后端 STT 对格式支持有限，需要在前端转码或在后端统一解码。~~ **已缓解**：Plan 1 实现中前端已统一输出 16kHz mono WAV 格式，绕过了 `webm/opus` 兼容问题。后端只需支持 WAV 解码即可。
- 风险 2：若直接把完整音频放进 JSON/Base64，请求体会显著膨胀；因此该方案只适合作为 MVP，不能直接承担长语音和低延迟场景。
- 风险 3：Barge-In 与 TTS 回声抑制在真实设备上会有误判，需要通过状态机和超时策略限制”误打断”和”重复听写”。Plan 1 已实现两阶段验证（候选期 150ms + 验证期 120ms）降低误触率，后续需在真实设备上验证效果。
- 风险 4：若 STT、LLM、TTS 来自不同提供方，整体超时预算和错误归因会变复杂，需要统一 trace/request_id 贯穿。
- 待定项 1：STT/TTS 是否接第三方云服务、开源本地模型，还是两者并存；这会影响后端适配层抽象。
- 待定项 2：语音对话是否要求”只保留文本落库”还是”文本+音频元信息落库”；这会影响会话模型和审计设计。
- ~~待定项 3：首版是否支持”按住说话”和”免按住连续对话”两种交互。~~ **已决定**：Plan 1 实现为点击式语音输入（非按住说话），连续对话通过 `voiceModeActive` 控制 TTS 播放结束后自动续录。
