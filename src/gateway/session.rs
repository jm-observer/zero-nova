use crate::message::{ContentBlock, Message, Role};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::gateway::protocol::{ContentBlockDTO, MessageDTO, Session as SessionProtocol};

/// 单个会话的详细信息与状态
pub struct Session {
    // Control layer state (Phase 4)
    pub control: std::sync::RwLock<crate::gateway::control::ControlState>,
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,       // unix timestamp in milliseconds
    pub updated_at: AtomicI64, // 支持按活跃度排序
    pub chat_lock: Mutex<()>,  // 确保同一会话内的聊天请求串行执行
    pub cancellation_token: RwLock<Option<CancellationToken>>,
}

impl Session {
    /// 追加 user message 到历史
    pub fn append_user_message(&self, input: &str) {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: input.to_string(),
            }],
        };
        let mut history = self.history.write().unwrap();
        history.push(msg);
        self.touch_updated_at();
    }

    /// 追加 assistant 返回的消息
    pub fn append_assistant_messages(&self, msgs: Vec<Message>) {
        let mut history = self.history.write().unwrap();
        history.extend(msgs);
        self.touch_updated_at();
    }

    /// 获取完整历史
    pub fn get_history(&self) -> Vec<Message> {
        self.history.read().unwrap().clone()
    }

    /// 获取 DTO 格式的历史
    pub fn get_messages_dto(&self) -> Vec<MessageDTO> {
        let history = self.history.read().unwrap();
        history
            .iter()
            .map(|m| MessageDTO {
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m
                    .content
                    .iter()
                    .map(|c| match c {
                        ContentBlock::Text { text } => ContentBlockDTO::Text { text: text.clone() },
                        ContentBlock::Thinking { thinking } => ContentBlockDTO::Thinking {
                            thinking: thinking.clone(),
                        },
                        ContentBlock::ToolUse { id, name, input } => ContentBlockDTO::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        },
                        ContentBlock::ToolResult {
                            tool_use_id,
                            output,
                            is_error,
                        } => ContentBlockDTO::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: output.clone(),
                            is_error: *is_error,
                        },
                    })
                    .collect(),
                timestamp: self.created_at, // 简单处理，目前 Message 结构没带时间戳
            })
            .collect()
    }

    /// 更新 updated_at 为当前时间
    pub fn touch_updated_at(&self) {
        self.updated_at.store(Utc::now().timestamp_millis(), Ordering::SeqCst);
    }

    /// 设置 cancellation token
    pub fn set_cancellation_token(&self, token: CancellationToken) {
        let mut ct = self.cancellation_token.write().unwrap();
        *ct = Some(token);
    }

    /// 清除 cancellation token
    pub fn clear_cancellation_token(&self) {
        let mut ct = self.cancellation_token.write().unwrap();
        *ct = None;
    }

    /// 获取并清除 cancellation token
    pub fn take_cancellation_token(&self) -> Option<CancellationToken> {
        let mut ct = self.cancellation_token.write().unwrap();
        ct.take()
    }
}

/// 内存会话存储库
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore {
    /// 创建新的 SessionStore
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// 创建一个新会话
    pub async fn create(&self, name: Option<String>, agent_id: String, system_prompt: String) -> Arc<Session> {
        let id = Uuid::new_v4().to_string();
        self.create_with_id(id, name, agent_id, system_prompt).await
    }

    pub async fn create_with_id(
        &self,
        id: String,
        name: Option<String>,
        agent_id: String,
        system_prompt: String,
    ) -> Arc<Session> {
        let length = if id.len() > 8 { 8 } else { id.len() };
        let session_name = name.unwrap_or_else(|| format!("Session {}", &id[..length]));
        let now = Utc::now().timestamp_millis();

        let mut initial_history = Vec::new();
        if !system_prompt.is_empty() {
            initial_history.push(Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: system_prompt }],
            });
        }

        let session = Arc::new(Session {
            control: std::sync::RwLock::new(crate::gateway::control::ControlState::new(&agent_id)),
            id: id.clone(),
            name: session_name,
            system_prompt,
            history: RwLock::new(initial_history),
            created_at: now,
            updated_at: AtomicI64::new(now),
            chat_lock: Mutex::new(()),
            cancellation_token: RwLock::new(None),
        });

        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id, session.clone());
        session
    }

    /// 根据 ID 获取会话
    pub async fn get(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(id).cloned()
    }

    /// 按 updated_at 降序返回会话摘要列表
    pub async fn list_sorted(&self) -> Vec<SessionProtocol> {
        let sessions = self.sessions.read().unwrap();
        let mut list: Vec<_> = sessions.values().cloned().collect();

        list.sort_by(|a, b| {
            b.updated_at
                .load(Ordering::SeqCst)
                .cmp(&a.updated_at.load(Ordering::SeqCst))
        });

        list.into_iter()
            .map(|s| SessionProtocol {
                id: s.id.clone(),
                title: Some(s.name.clone()),
                agent_id: s.control.read().unwrap().active_agent.clone(),
                created_at: s.created_at,
                updated_at: s.updated_at.load(Ordering::SeqCst),
                message_count: s.history.read().unwrap().len(),
            })
            .collect()
    }

    /// 删除会话
    pub async fn delete(&self, id: &str) -> bool {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(id).is_some()
    }
}
