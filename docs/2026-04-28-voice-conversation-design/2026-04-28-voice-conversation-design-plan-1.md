# Plan 1：前端语音交互与状态机收敛

## 前置依赖
- 无

## 本次目标
- 基于现有 `deskapp/src/voice.ts` 和 `deskapp/src/ui/voice-overlay.ts`，定义前端语音闭环所需的状态机、事件流与 UI 反馈。
- 明确 MVP 与后续流式模式下前端分别需要承担的职责边界。

## 涉及文件
- `deskapp/src/voice.ts`
- `deskapp/src/ui/voice-overlay.ts`
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/core/event-bus.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/main.ts`

## 详细设计

### 1. 复用现有能力，不推翻 `voice.ts`
- 当前 `voice.ts` 已经具备：
  - 录音开始/停止
  - VAD 自动停录
  - TTS 队列播放
  - 分句流式 TTS
  - Barge-In 打断检测
- 因此前端方案不建议新建第二套语音控制器，而是把 `voice.ts` 作为统一语音域服务，向外暴露更稳定的高层 API。

### 2. 统一前端语音状态机
- 建议增加统一状态枚举：
  - `idle`
  - `requesting_permission`
  - `recording`
  - `uploading_audio`
  - `recognizing`
  - `submitting_text`
  - `waiting_assistant`
  - `speaking`
  - `interrupted`
  - `error`
- 状态切换原则：
  - UI 浮层、聊天输入框、快捷键、播放按钮都只读写这一套状态。
  - 所有异步步骤都要有超时、取消和错误出口，避免 UI 悬挂在“处理中”。

### 3. MVP 交互流程
- 用户点击语音按钮或按住快捷键。
- 前端进入 `requesting_permission`，成功后进入 `recording`。
- 录音结束条件：
  - 手动停止；
  - VAD 静音超时自动停止；
  - 达到最大时长自动停止。
- 停止录音后：
  - 前端生成音频 Blob/ArrayBuffer；
  - 进入 `uploading_audio`；
  - 调用 Gateway `voice.transcribe`；
  - 收到转写后进入 `submitting_text`；
  - 将识别文本显示在输入框或作为临时用户消息；
  - 继续调用现有聊天发送流程；
  - 助手回复完成后，若开启 `ttsAutoPlay`，进入 `speaking` 并调用 TTS。

### 4. 连续语音模式
- 现有 `voiceModeActive` 可用于“连续对话模式”。
- 连续模式不建议首版就做真双工；推荐首版实现“单轮自动续录”：
  - TTS 播放结束后回到 `idle`；
  - 若用户仍处于语音模式，再自动进入下一轮 `recording`。
- 这样可以复用已有的 VAD 与 Barge-In 设计，同时减少协议复杂度。

### 5. Barge-In 策略
- 当前 `voice.ts` 已有 Barge-In 检测，建议语义上只做两件事：
  - 停止当前 TTS 播放；
  - 根据配置选择“直接开始新录音”或“仅停止播报，等待用户再次点击”。
- 首版建议默认“停止播报但不立即重新录音”，降低误触发带来的体验问题。

### 6. UI 呈现建议
- 语音浮层需要展示：
  - 当前状态文案
  - 实时录音时长
  - 转写中的加载态
  - 识别文本预览
  - 错误提示与重试入口
- 聊天主时间线建议增加两类临时消息：
  - `voice_transcript_pending`
  - `voice_transcript_final`
- 这样即便识别或聊天失败，用户也能看到本轮输入到底走到了哪一步。

### 7. 与聊天发送链路的衔接
- 不建议为语音再造一条独立“问答”链路。
- 推荐将 STT 的最终产物仍然收敛为现有文本消息发送接口，以保证：
  - 会话持久化逻辑不分叉；
  - 现有消息列表、资源面板、调试台可以继续复用；
  - 语音与文本共享上下文、权限、MCP 与会话管理能力。

## 测试案例
- 正常路径：录音 -> 上传 -> 识别成功 -> 文本消息发出 -> 助手回复 -> 自动朗读。
- 边界条件：空白音频、录音时长过短、录音达到最大时长、用户在识别过程中取消。
- 异常场景：麦克风权限被拒、Gateway 断线、STT 超时、TTS 合成失败、Barge-In 误触发。
- 回归场景：文本聊天仍可正常使用；关闭语音模式后现有 UI 行为不变。

