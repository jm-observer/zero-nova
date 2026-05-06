# Plan 2：Gateway 语音协议与传输设计

## 前置依赖
- Plan 1：前端语音交互与状态机收敛

## 本次目标
- 设计适用于 MVP 和后续流式扩展的语音协议。
- 明确哪些消息走 JSON 控制帧，哪些数据适合走二进制音频帧。

## 涉及文件
- `crates/nova-protocol/src/envelope.rs`
- `crates/nova-protocol/src/lib.rs`
- `schemas/domains/gateway/`
- `deskapp/src/gateway-client.ts`
- `deskapp/src/gateway-messages.ts`
- `deskapp/src/generated/generated-types.ts`

## 详细设计

### 1. 协议设计原则
- 文本会话协议继续保持现状，语音能力作为独立 capability 叠加，不破坏现有 chat 流程。
- 所有语音相关消息都带 `requestId` / `sessionId` / `turnId` 或等价关联标识，便于串联 STT、聊天和 TTS 全链路日志。
- 区分“能力发现”“控制消息”“数据消息”“结果消息”“错误消息”。

### 2. MVP 消息模型

> **注意**：以下协议语义名与已实现的 Rust/TypeScript 类型名的对应关系见末尾"命名对照表"。

- 已定义的请求/事件语义：
  - `voice.capabilities.get` → `VoiceCapabilitiesRequest` / `VoiceCapabilitiesResponse`
  - `voice.transcribe.request` → `VoiceTranscribeRequest` / `VoiceTranscribeResponse`
  - `voice.tts.request` → `VoiceTtsRequest` / `VoiceTtsResponse`
  - `voice.error`（待实现）
- `voice.transcribe.request`（Rust: `VoiceTranscribeRequest`）字段：
  - `session_id?: String`
  - `audio_format: String` — MVP 固定为 `"wav"`（前端输出 16kHz mono WAV）
  - `sample_rate?: u32` — 默认 16000
  - `channel_count?: u16` — 默认 1
  - `language?: String`
  - `mode: VoiceConversationMode` — 当前仅 `Once`
  - `audio_base64: String` — Base64 编码的 WAV 数据，**上限 10MB（编码后约 13.3MB Base64）**
- `voice.transcribe.response`（Rust: `VoiceTranscribeResponse`）字段：
  - `text: String`
  - `confidence?: f32`
  - `duration_ms?: u64`
  - `segments: Vec<VoiceSegment>` — 每段包含 `start_ms`、`end_ms`、`text`
- `voice.tts.request`（Rust: `VoiceTtsRequest`）字段：
  - `text: String` — 需要合成的文本
  - `voice?: String` — 语音角色，可选，不指定时使用配置默认值
  - `session_id?: String` — 关联会话 ID
- `voice.tts.response`（Rust: `VoiceTtsResponse`）字段：
  - `audio_format: String` — 返回的音频格式（如 `"mp3"`）
  - `audio_base64: String` — Base64 编码的音频数据

### 3. 流式扩展消息模型
- 第二阶段建议新增：
  - `voice.session.start`
  - `voice.audio.chunk`
  - `voice.audio.commit`
  - `voice.audio.cancel`
  - `voice.transcript.partial`
  - `voice.transcript.final`
  - `voice.reply.partial`
  - `voice.reply.final`
  - `voice.tts.chunk`
  - `voice.session.end`
- 其中：
  - JSON 帧只承载元数据和控制指令；
  - 音频片段优先走二进制帧，并通过帧头或关联字段绑定 `voiceSessionId` 与序号。

### 4. 为什么推荐“两阶段传输”
- MVP 若直接引入双工流式二进制帧，前端状态机、Gateway 分发和服务端编排会同时变复杂。
- 先做“整段上传”可以更快验证：
  - 语音按钮交互是否成立；
  - STT 准确率是否可接受；
  - TTS 自动朗读是否符合产品预期；
  - 用户是否真的需要更低延迟。
- 一旦产品验证通过，再在协议层平滑升级到流式分片，不需要推翻上层业务语义。

### 5. 错误与可观测性
- 建议所有语音错误统一使用独立 capability，例如 `capability: 'voice.stt' | 'voice.tts' | 'voice.transport'`。
- 典型错误码：
  - `voice_input_too_short`
  - `voice_format_unsupported`
  - `voice_decode_failed`
  - `voice_stt_timeout`
  - `voice_stt_unavailable`
  - `voice_tts_timeout`
  - `voice_tts_unavailable`
- 语音事件需纳入现有 Gateway 调试流，方便在桌面端调试台中看到本轮语音调用轨迹。

### 6. 能力发现触发时机
- `voice.capabilities.get` 由**前端主动请求**（通过 `gatewayClient.getVoiceCapabilities()`），而非 Gateway 推送。
- 建议在以下时机调用：
  - 前端首次建立 WebSocket 连接后；
  - 用户打开语音设置面板时；
  - 连接断开重连后。
- 前端根据返回的 `stt.available` / `tts.available` 控制语音入口的显示/隐藏。

### 7. 数据格式建议
- MVP：前端统一输出 16kHz mono WAV（`audio_format: “wav”`），后端只需支持 WAV 解码。
- 流式阶段：建议前端转为单声道 PCM16 或 Opus 帧，减少后端兼容判断。
- 如果必须兼顾浏览器兼容与后端解码成本，可将”前端是否转 PCM”作为实现期开关，而不是协议强制要求。

### 8. 命名对照表

| 协议语义名 | Rust 类型（nova-protocol） | 前端方法（gateway-client.ts） |
|---|---|---|
| `voice.capabilities.get` | `VoiceCapabilitiesRequest` / `VoiceCapabilitiesResponse` | `getVoiceCapabilities()` |
| `voice.transcribe.request` | `VoiceTranscribeRequest` / `VoiceTranscribeResponse` | `transcribeVoice()` |
| `voice.tts.request` | `VoiceTtsRequest` / `VoiceTtsResponse` | `synthesizeVoice()` |

## 测试案例
- 正常路径：完整音频请求成功转写，返回文本和时长信息。
- 边界条件：不支持的 `audioFormat`、缺失 `audioBase64`、超大音频、重复 `requestId`。
- 异常场景：流式分片乱序、分片丢失、提交后继续发 chunk、Gateway 中途断连。
- 回归场景：原有文本协议 schema 与前端解析逻辑不受影响。

