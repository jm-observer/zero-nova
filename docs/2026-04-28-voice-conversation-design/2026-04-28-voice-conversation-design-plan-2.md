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
- 建议新增以下请求/事件语义：
  - `voice.capabilities.get`
  - `voice.transcribe.request`
  - `voice.transcribe.response`
  - `voice.tts.request`
  - `voice.tts.response`
  - `voice.error`
- `voice.transcribe.request` 建议字段：
  - `sessionId?: string`
  - `audioFormat: 'audio/webm;codecs=opus' | 'audio/webm' | 'audio/wav'`
  - `sampleRate?: number`
  - `channelCount?: number`
  - `language?: string`
  - `mode: 'once'`
  - `audioBase64: string`
- `voice.transcribe.response` 建议字段：
  - `text: string`
  - `confidence?: number`
  - `durationMs?: number`
  - `segments?: Array<{ startMs: number; endMs: number; text: string }>`

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

### 6. 数据格式建议
- MVP：允许 `audio/webm;codecs=opus` 直接上传，由后端统一解码。
- 流式阶段：建议前端转为单声道 PCM16 或 Opus 帧，减少后端兼容判断。
- 如果必须兼顾浏览器兼容与后端解码成本，可将“前端是否转 PCM”作为实现期开关，而不是协议强制要求。

## 测试案例
- 正常路径：完整音频请求成功转写，返回文本和时长信息。
- 边界条件：不支持的 `audioFormat`、缺失 `audioBase64`、超大音频、重复 `requestId`。
- 异常场景：流式分片乱序、分片丢失、提交后继续发 chunk、Gateway 中途断连。
- 回归场景：原有文本协议 schema 与前端解析逻辑不受影响。

