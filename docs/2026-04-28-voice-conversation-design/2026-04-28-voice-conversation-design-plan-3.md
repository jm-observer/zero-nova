# Plan 3：后端识别与应答编排

## 前置依赖
- Plan 2：Gateway 语音协议与传输设计

## 本次目标
- 在服务端建立统一语音编排层，串联 STT、文本对话和 TTS。
- 控制超时、重试、日志和降级行为，避免语音链路在多个模块内分散实现。

## 涉及文件
- `crates/nova-gateway-core/src/`
- `crates/nova-server/src/bin/nova_gateway_ws.rs`
- `crates/nova-agent/src/app/`
- `crates/nova-agent/src/config.rs`
- `crates/nova-protocol/src/`

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

### 3. 推荐的后端职责边界
- STT 只负责“音频 -> 文本”。
- 现有对话服务继续负责“文本 -> 助手回复”。
- TTS 只负责“文本 -> 音频”。
- 语音编排层负责串联这三步，并补充 trace、超时、取消、错误映射。

### 4. 超时与重试策略
- 建议显式配置常量或配置项，而不是散落 magic number：
  - `voice_stt_timeout_secs`
  - `voice_tts_timeout_secs`
  - `voice_max_audio_bytes`
  - `voice_max_audio_duration_secs`
  - `voice_partial_result_debounce_ms`
- STT/TTS 默认不建议自动重试超过 1 次：
  - 语音请求通常体积较大；
  - 重试会放大时延与费用；
  - 失败更适合尽快向前端暴露并允许用户重试。

### 5. 持久化与审计建议
- 建议首版只持久化：
  - 用户最终转写文本
  - 助手最终回复文本
  - 语音元信息（格式、时长、请求 ID、供应商、耗时）
- 不建议首版持久化原始音频内容，原因：
  - 存储成本和隐私风险更高；
  - 当前主要目标是验证语音交互闭环，而不是做语音资产留存。

### 6. 能力发现与降级
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

