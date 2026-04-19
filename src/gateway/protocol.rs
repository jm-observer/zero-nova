use crate::provider::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    // --- 3.2 System & Control Events ---
    #[serde(rename = "welcome")]
    Welcome(WelcomePayload),
    #[serde(rename = "auth.success")]
    AuthSuccess,
    #[serde(rename = "auth.failed")]
    AuthFailed(ErrorPayload),
    #[serde(rename = "error")]
    Error(ErrorPayload),

    // --- 2.1 Chat & Session Management ---
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
    #[serde(rename = "sessions.logs")]
    SessionsLogs(SessionIdPayload),
    #[serde(rename = "sessions.logs.response")]
    SessionsLogsResponse(LogListResponse),
    #[serde(rename = "sessions.artifacts")]
    SessionsArtifacts(SessionIdPayload),
    #[serde(rename = "sessions.artifacts.response")]
    SessionsArtifactsResponse(ArtifactListResponse),

    // --- 3.1 Progress & Chat Events ---
    #[serde(rename = "chat.start")]
    ChatStart(SessionIdPayload),
    #[serde(rename = "chat.intent")]
    ChatIntent(ChatIntentPayload),
    #[serde(rename = "chat.progress")]
    ChatProgress(ProgressEvent),
    #[serde(rename = "chat.complete")]
    ChatComplete(ChatCompletePayload),

    // --- Interaction Events ---
    #[serde(rename = "interaction.request")]
    InteractionRequest(InteractionRequestPayload),
    #[serde(rename = "interaction.resolved")]
    InteractionResolved(InteractionResolvedPayload),

    // --- 2.2 Agent Management ---
    #[serde(rename = "agents.list")]
    AgentsList,
    #[serde(rename = "agents.list.response")]
    AgentsListResponse(AgentsListResponse),
    #[serde(rename = "agents.create")]
    AgentsCreate(AgentCreateRequest),
    #[serde(rename = "agents.switch")]
    AgentsSwitch(AgentIdPayload),
    #[serde(rename = "agents.switch.response")]
    AgentsSwitchResponse(AgentsSwitchResponse),

    // --- 2.3 Scheduler API ---
    #[serde(rename = "scheduler.list")]
    SchedulerList,
    #[serde(rename = "scheduler.list.response")]
    SchedulerListResponse(Value),
    #[serde(rename = "scheduler.trigger")]
    SchedulerTrigger(TaskIdPayload),

    // --- 2.4 Memory & Distillation ---
    #[serde(rename = "memory.stats")]
    MemoryStats,
    #[serde(rename = "memory.stats.response")]
    MemoryStatsResponse(Value),

    // --- 2.5 System & Integration ---
    #[serde(rename = "auth")]
    Auth(AuthRequest),
    #[serde(rename = "config.get")]
    ConfigGet,
    #[serde(rename = "config.get.response")]
    ConfigGetResponse(Value),
    #[serde(rename = "config.get-llm-source")]
    ConfigGetLlmSource,
    #[serde(rename = "config.get-llm-source.response")]
    ConfigGetLlmSourceResponse(Value),
    #[serde(rename = "settings.get")]
    SettingsGet,
    #[serde(rename = "browser.launch")]
    BrowserLaunch,
    #[serde(rename = "browser.status")]
    BrowserStatus,
    #[serde(rename = "browser.status.response")]
    BrowserStatusResponse(Value),

    // --- Router & Weixin ---
    #[serde(rename = "router.config.get")]
    RouterConfigGet,
    #[serde(rename = "router.status")]
    RouterStatus(ConnectStatusPayload),
    #[serde(rename = "weixin.config.get")]
    WeixinConfigGet,
    #[serde(rename = "weixin.config.update")]
    WeixinConfigUpdate(Value),
    #[serde(rename = "weixin.status")]
    WeixinStatus(ConnectStatusPayload),

    // --- Voice ---
    #[serde(rename = "voice.get-status")]
    VoiceGetStatus,
    #[serde(rename = "voice.get-status.response")]
    VoiceGetStatusResponse(Value),

    // --- OpenFlux Cloud ---
    #[serde(rename = "openflux.status")]
    OpenFluxStatus,
    #[serde(rename = "openflux.status.response")]
    OpenFluxStatusResponse(Value),
    #[serde(rename = "language.update")]
    LanguageUpdate(LanguageUpdatePayload),
    #[serde(rename = "language.update.response")]
    LanguageUpdateResponse(Value),

    #[serde(rename = "router.config.update")]
    RouterConfigUpdate(RouterConfigUpdatePayload),

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouterConfigUpdatePayload {
    pub app_user_id: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogListResponse {
    pub logs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactListResponse {
    pub artifacts: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LanguageUpdatePayload {
    pub language: String,
}

// --- Interaction protocol structs ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOptionDTO {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionRequestPayload {
    pub session_id: String,
    pub interaction_id: String,
    pub kind: String, // "approve" | "select" | "input"
    pub subject: String,
    pub prompt: String,
    pub options: Vec<InteractionOptionDTO>,
    pub risk_level: String, // "low" | "medium" | "high"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionResolvedPayload {
    pub session_id: String,
    pub interaction_id: String,
    pub result: String, // "approved" | "rejected" | "selected" | "input" | "expired"
}

// --- Payload Definitions ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WelcomePayload {
    pub require_auth: bool,
    pub setup_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayload {
    pub input: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub attachments: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String, // 'thinking' | 'tool_start' | 'tool_result' | 'token' | 'complete'
    pub session_id: Option<String>,
    pub iteration: Option<i32>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub is_error: Option<bool>,
    pub thinking: Option<String>,
    pub token: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatIntentPayload {
    pub session_id: String,
    pub intent: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletePayload {
    pub session_id: String,
    pub output: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateRequest {
    pub title: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreateRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdPayload {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdPayload {
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdPayload {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectStatusPayload {
    pub connected: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuccessResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionsListResponse {
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionsMessagesResponse {
    pub messages: Vec<MessageDTO>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateResponse {
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentsListResponse {
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentsSwitchResponse {
    pub agent: Agent,
    pub messages: Vec<Value>,
}

// --- Data Models ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: Option<String>,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDTO {
    pub role: String,
    pub content: Vec<ContentBlockDTO>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlockDTO {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub system_prompt: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_event() {
        let msg = GatewayMessage::new_event(MessageEnvelope::Welcome(WelcomePayload {
            require_auth: true,
            setup_required: false,
        }));
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"welcome\""));
        assert!(json.contains("\"requireAuth\":true"));
        assert!(!json.contains("\"id\":"));
    }

    #[test]
    fn test_serialize_request() {
        let msg = GatewayMessage::new(
            "req-1".to_string(),
            MessageEnvelope::Chat(ChatPayload {
                input: "hello".into(),
                session_id: None,
                agent_id: None,
                attachments: None,
            }),
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"id\":\"req-1\""));
        assert!(json.contains("\"type\":\"chat\""));
        assert!(json.contains("\"payload\":{"));
    }

    #[test]
    fn test_serialize_unit_variant() {
        let msg = GatewayMessage::new("req-2".to_string(), MessageEnvelope::SessionsList);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"sessions.list\""));
        assert!(!json.contains("\"payload\""));
    }

    #[test]
    fn test_deserialize_progress() {
        let json = r#"{
            "type": "chat.progress",
            "payload": {
                "type": "token",
                "token": "Hello",
                "sessionId": "s1"
            }
        }"#;
        let msg: GatewayMessage = serde_json::from_str(json).unwrap();
        if let MessageEnvelope::ChatProgress(p) = msg.envelope {
            assert_eq!(p.kind, "token");
            assert_eq!(p.token.unwrap(), "Hello");
            assert_eq!(p.session_id.unwrap(), "s1");
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_deserialize_agents_switch() {
        let json = r#"{
            "type": "agents.switch",
            "id": "req-3",
            "payload": {
                "agentId": "nova"
            }
        }"#;
        let msg: GatewayMessage = serde_json::from_str(json).unwrap();
        if let MessageEnvelope::AgentsSwitch(payload) = msg.envelope {
            assert_eq!(payload.agent_id, "nova");
        } else {
            panic!("Wrong variant");
        }
    }
}
