use crate::agent::AgentRuntime;
use crate::gateway::handlers::system::send_general_error;
use crate::gateway::protocol::Session;
use crate::gateway::protocol::{
    ChatCompletePayload, ChatPayload, ErrorPayload, GatewayMessage, MessageEnvelope, SessionIdPayload,
};
use crate::gateway::session::SessionStore;
use crate::provider::LlmClient;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::gateway::router::AppState;

pub async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let session_id = match payload.session_id {
        Some(id) => id,
        None => {
            send_general_error(
                &outbound_tx,
                &request_id,
                "session id not found".to_string(),
                Some("SESSION_ID_NOT_FOUND".to_string()),
            );
            return;
        }
    };

    let session = match state.sessions.get(&session_id).await {
        Some(s) => s,
        None => state.sessions.create_with_id(session_id.clone(), None).await,
    };

    let _lock = session.chat_lock.lock().await;

    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.clone(),
        MessageEnvelope::ChatStart(SessionIdPayload {
            session_id: session_id.clone(),
        }),
    ));

    // 2. 创建事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session_id.clone();

    // 3. Spawn 转发任务
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // 4. 准备历史上下文
    let history = {
        let h = session.history.read().unwrap();
        h.clone()
    };

    // 5. 调用 agent.run_turn
    match state.agent.run_turn(&history, &payload.input, event_tx, None).await {
        Ok(new_messages) => {
            let mut h = session.history.write().unwrap();
            let user_msg = crate::message::Message {
                role: crate::message::Role::User,
                content: vec![crate::message::ContentBlock::Text { text: payload.input }],
            };
            h.push(user_msg);
            h.extend(new_messages.clone());

            // 6. 发送 chat.complete
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id.clone(),
                MessageEnvelope::ChatComplete(ChatCompletePayload {
                    session_id: session_id.clone(),
                    output: None, // 暂时不传 output，由前端从 history 获取
                    usage: None,  // 暂未在 run_turn 返回中包含 usage，后续需要完善
                }),
            ));
        }
        Err(e) => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Agent execution error: {}", e),
                Some("AGENT_EXECUTION_ERROR".to_string()),
            );
        }
    }

    let _ = bridge_handle.await;
}
