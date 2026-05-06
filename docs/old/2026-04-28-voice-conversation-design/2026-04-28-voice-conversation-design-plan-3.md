# Plan 3：后端识别与应答编排

## 前置依赖
- Plan 2：Gateway 语音协议与传输设计

## 本次目标
- 在服务端建立统一语音编排层，串联 STT、文本对话和 TTS。
- 控制超时、重试、日志和降级行为，避免语音链路在多个模块内分散实现。

## 涉及文件
- `crates/nova-gateway-core/src/handlers/mod.rs` — 注册语音 handler
- `crates/nova-gateway-core/src/handlers/voice.rs` — **新增**，语音请求 handler（传输适配层）
- `crates/nova-gateway-core/src/router.rs` — 路由表增加语音消息分发
- `crates/nova-agent/src/app/mod.rs` — Application facade 暴露语音编排入口
- `crates/nova-agent/src/app/voice_service.rs` — **新增**，语音编排层（串联 STT → LLM → TTS）
- `crates/nova-agent/src/voice/mod.rs` — **新增**，供应商适配层 trait 定义
- `crates/nova-agent/src/voice/openai_compat.rs` — **新增**，OpenAI 兼容 STT/TTS 供应商实现
- `crates/nova-agent/src/voice/mock.rs` — **新增**，mock 供应商（用于测试和灰度）
- `crates/nova-agent/src/config.rs` — `VoiceConfig` 已存在，需补充供应商选择字段
- `crates/nova-protocol/src/voice.rs` — 已有基础类型，按需扩展错误类型
- `crates/nova-server/src/bin/nova_gateway_ws.rs` — 如需调整 WebSocket handler 签名

## 详细设计

### 1. 后端分层建议
- 建议新增语音编排边界，逻辑分三层：
  - 传输适配层：接收 Gateway 语音请求、校验格式、处理分片缓存。
  - 语音应用层：负责一次完整语音轮次编排，调用 STT/LLM/TTS。
  - 供应商适配层：封装具体 STT/TTS 提供方或本地模型。
- 这样可以避免把第三方语音接口细节直接散落在 Gateway handler 中。

### 2. MVP 编排流程
- 接收 `voice.transcribe.request`。
- 校验大小、格式、会话上下文。
- 解码音频并调用 STT。
- 将 STT 结果作为普通用户文本输入交给现有聊天服务。
- 获得助手最终文本后：
  - 始终返回文本结果；
  - 若前端请求 TTS 或用户配置开启自动朗读，再调用 TTS。
- 将 TTS 音频作为单独响应返回，避免把“聊天完成”和“语音合成完成”强耦合到一个超长请求中。

### 3. 供应商适配层 trait 定义

```rust
/// STT 供应商 trait
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// 将音频数据转写为文本
    async fn transcribe(
        &self,
        audio: &[u8],
        format: &str,          // e.g. “wav”
        language: Option<&str>,
    ) -> Result<TranscribeResult>;
}

pub struct TranscribeResult {
    pub text: String,
    pub confidence: Option<f32>,
    pub duration_ms: Option<u64>,
    pub segments: Vec<VoiceSegment>,
}

/// TTS 供应商 trait
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// 将文本合成为音频
    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
    ) -> Result<SynthesizeResult>;
}

pub struct SynthesizeResult {
    pub audio: Vec<u8>,
    pub audio_format: String,  // e.g. “mp3”
}
```

- 首版提供两个实现：
  - `OpenAiCompatSttProvider` / `OpenAiCompatTtsProvider`：兼容 OpenAI API 格式（whisper-1 / tts-1），可对接本地或第三方服务
  - `MockSttProvider` / `MockTtsProvider`：返回固定文本/静音音频，用于测试和灰度

### 4. 推荐的后端职责边界
- STT 只负责”音频 -> 文本”。
- 现有对话服务继续负责”文本 -> 助手回复”。
- TTS 只负责”文本 -> 音频”。
- 语音编排层负责串联这三步，并补充 trace、超时、取消、错误映射。

### 5. TTS 与聊天回复的时序关系
- MVP 采用**独立请求模式**：STT 和 TTS 是两个独立的请求/响应周期，不在同一个请求中耦合。
- 具体流程：
  1. 前端发送 `voice.transcribe.request` → 后端返回转写文本（`voice.transcribe.response`）
  2. 前端将转写文本作为普通用户消息发送 → 后端返回助手回复（现有 chat 流程）
  3. 前端在助手回复完成后，若开启自动朗读，**单独调用** `voice.tts.request` → 后端返回合成音频
- 三步请求共享同一个 `sessionId`，便于日志串联，但彼此独立，任一步失败不阻塞其他步骤的结果。
- 前端通过 `synthesizeVoice()` 独立发起 TTS 请求（已在 `gateway-client.ts` 中实现）。

### 6. 并发控制
- 同一用户的语音请求应串行处理：新的 `voice.transcribe.request` 到达时，若前一轮仍在处理中：
  - 首版建议**拒绝并返回错误**（`voice_request_in_progress`），由前端控制发送时序。
  - 不建议首版做自动取消前一轮，因为取消逻辑会引入状态竞争。
- TTS 请求不受此限制（因为 TTS 是幂等的，且可能需要重试）。
- 连续语音模式下，前端负责在前一轮完全结束后（状态回到 `idle`）才发起下一轮录音。

### 7. 超时与重试策略
- `VoiceConfig`（`crates/nova-agent/src/config.rs`）已定义以下配置项：
  - `stt_timeout_ms`（默认值待确认）— STT 请求超时
  - `tts_timeout_ms`（默认值待确认）— TTS 请求超时
  - `max_input_bytes`（默认值待确认）— 音频最大输入字节数
- 建议补充的配置项（如需要）：
  - `voice_max_audio_duration_secs` — 最大录音时长（秒）
  - `voice_partial_result_debounce_ms` — 流式阶段增量结果防抖间隔
- STT/TTS 默认不建议自动重试超过 1 次：
  - 语音请求通常体积较大；
  - 重试会放大时延与费用；
  - 失败更适合尽快向前端暴露并允许用户重试。

### 8. 持久化与审计建议
- 建议首版只持久化：
  - 用户最终转写文本
  - 助手最终回复文本
  - 语音元信息（格式、时长、请求 ID、供应商、耗时）
- 不建议首版持久化原始音频内容，原因：
  - 存储成本和隐私风险更高；
  - 当前主要目标是验证语音交互闭环，而不是做语音资产留存。

### 9. 能力发现与降级
- Gateway 应提供 `voice.capabilities.get` 或在 welcome/capabilities 中声明：
  - 是否支持 STT
  - 是否支持 TTS
  - 支持的输入编码
  - 支持的语音角色列表
- 前端可据此：
  - 隐藏不可用入口；
  - 禁用自动朗读；
  - 提示需要下载模型或配置供应商。

## 测试案例
- 正常路径：音频识别成功、文本会话成功、TTS 成功返回音频。
- 边界条件：超长音频、空转写、识别结果置信度过低、会话不存在。
- 异常场景：STT 供应商超时、TTS 供应商失败、聊天链路失败、取消请求到达时任务正在运行。
- 回归场景：纯文本聊天性能和行为不因语音能力接入而改变。

