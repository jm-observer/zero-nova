use crate::agent::{AgentsListResponse, AgentsSwitchResponse, SessionAgentSwitchPayload};
use crate::chat::{ChatCompletePayload, ChatIntentPayload, ChatPayload, ProgressEvent};
use crate::session::{
    SessionCopyRequest, SessionCreateRequest, SessionCreateResponse, SessionIdPayload, SessionsListResponse,
    SessionsMessagesResponse, SuccessResponse,
};
use crate::system::{ErrorPayload, WelcomePayload};
use serde::{Deserialize, Serialize};

/// 统一消息信封 (与 OpenFlux 前端规范完全一致)
/// 序列化结构: { "id": "...", "type": "...", "payload": { ... } }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// 请求响应模式下的消息 ID，事件推送时为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}

impl GatewayMessage {
    /// 创建一个新的请求或响应消息
    pub fn new(id: String, envelope: MessageEnvelope) -> Self {
        Self { id: Some(id), envelope }
    }

    /// 创建一个服务器主动推送的事件消息
    pub fn new_event(envelope: MessageEnvelope) -> Self {
        Self { id: None, envelope }
    }
}

/// 消息封装，包含类型标签和可选的 Payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum MessageEnvelope {
    // --- System & Control Events ---
    #[serde(rename = "error")]
    Error(ErrorPayload),
    #[serde(rename = "welcome")]
    Welcome(WelcomePayload),

    // --- Chat & Session Management ---
    #[serde(rename = "chat")]
    Chat(ChatPayload),
    #[serde(rename = "chat.stop")]
    ChatStop(SessionIdPayload),
    #[serde(rename = "chat.stop.response")]
    ChatStopResponse(SessionIdPayload),
    #[serde(rename = "sessions.list")]
    SessionsList,
    #[serde(rename = "sessions.list.response")]
    SessionsListResponse(SessionsListResponse),
    #[serde(rename = "sessions.messages")]
    SessionsMessages(SessionIdPayload),
    #[serde(rename = "sessions.messages.response")]
    SessionsMessagesResponse(SessionsMessagesResponse),
    #[serde(rename = "sessions.create")]
    SessionsCreate(SessionCreateRequest),
    #[serde(rename = "sessions.create.response")]
    SessionsCreateResponse(SessionCreateResponse),
    #[serde(rename = "sessions.delete")]
    SessionsDelete(SessionIdPayload),
    #[serde(rename = "sessions.delete.response")]
    SessionsDeleteResponse(SuccessResponse),
    #[serde(rename = "sessions.copy")]
    SessionsCopy(SessionCopyRequest),
    #[serde(rename = "sessions.copy.response")]
    SessionsCopyResponse(SessionCreateResponse),

    // --- Progress & Chat Events ---
    #[serde(rename = "chat.start")]
    ChatStart(SessionIdPayload),
    #[serde(rename = "chat.intent")]
    ChatIntent(ChatIntentPayload),
    #[serde(rename = "chat.progress")]
    ChatProgress(ProgressEvent),
    #[serde(rename = "chat.complete")]
    ChatComplete(ChatCompletePayload),

    // --- Agent Management ---
    #[serde(rename = "agents.list")]
    AgentsList,
    #[serde(rename = "agents.list.response")]
    AgentsListResponse(AgentsListResponse),
    #[serde(rename = "agents.switch")]
    AgentsSwitch(SessionAgentSwitchPayload),
    #[serde(rename = "agents.switch.response")]
    AgentsSwitchResponse(AgentsSwitchResponse),

    // --- System & Integration ---
    #[serde(rename = "config.get")]
    ConfigGet,
    #[serde(rename = "config.get.response")]
    ConfigGetResponse(serde_json::Value),
    #[serde(rename = "config.update")]
    ConfigUpdate(serde_json::Value),
    #[serde(rename = "config.update.response")]
    ConfigUpdateResponse(SuccessResponse),

    #[serde(other)]
    Unknown,
}
