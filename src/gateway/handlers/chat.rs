use crate::gateway::handlers::system::{send_general_error, send_general_error_direct};
use crate::gateway::protocol::{
    ChatCompletePayload, ChatPayload, GatewayMessage, MessageEnvelope, SessionIdPayload,
};
use crate::provider::LlmClient;
use log::error;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::gateway::router::AppState;

pub async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let session = state.sessions.get_or_create(payload.session_id.clone()).await;
    let session_id = session.id.clone();

    let _lock = session.chat_lock.lock().await;

    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.clone(),
        MessageEnvelope::ChatStart(SessionIdPayload {
            session_id: session_id.clone(),
        }),
    ));

    // 2. 写入 User Message (Step 3 conversion: 预写入以防失败丢失)
    session.append_user_message(&payload.input);

    // 3. 创建并注册 CancellationToken
    let token = CancellationToken::new();
    session.set_cancellation_token(token.clone());

    // 4. 创建事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session_id.clone();

    // 5. Spawn 转发任务
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // 6. 准备历史上下文 (包含刚刚写入的 user message)
    let history = session.get_history();

    // 7. 调用 agent.run_turn
    // 注意：history 里面已经包含了当前用户输入，但 run_turn 内部还会再次 push。
    // 为了修复这个问题，我们需要从 history 中剥离最后一条 user message 传给 run_turn，
    // 或者修改 run_turn 不再自己 push。
    // 根据 phase3-session-management.md，run_turn 的签名是 run_turn(history, input, ...)。
    // 如果 history 包含了 input，则会重复。
    // 解决方法：传入 history[..history.len()-1]
    let history_for_turn = &history[..history.len() - 1];

    match state
        .agent
        .run_turn(history_for_turn, &payload.input, event_tx, Some(token))
        .await
    {
        Ok(turn_result) => {
            session.append_assistant_messages(turn_result.messages);

            // 8. 发送 chat.complete
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id.clone(),
                MessageEnvelope::ChatComplete(ChatCompletePayload {
                    session_id: session_id.clone(),
                    output: None,
                    usage: Some(turn_result.usage),
                }),
            ));
        }
        Err(e) => {
            error!("Agent execution error for session {}: {}", session_id, e);
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Agent execution error: {}", e),
                Some("AGENT_EXECUTION_ERROR".to_string()),
            );
        }
    }

    session.clear_cancellation_token();
    session.touch_updated_at();
    let _ = bridge_handle.await;
}

pub async fn handle_chat_stop<C: LlmClient>(
    payload: SessionIdPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(session) = state.sessions.get(&payload.session_id).await {
        if let Some(token) = session.take_cancellation_token() {
            token.cancel();
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ChatStopResponse(SessionIdPayload {
                    session_id: payload.session_id,
                }),
            ));
        } else {
            send_general_error_direct(
                &outbound_tx,
                &request_id,
                "No active chat to stop".to_string(),
                Some("NO_ACTIVE_CHAT".to_string()),
            );
        }
    } else {
        send_general_error_direct(
            &outbound_tx,
            &request_id,
            "Session not found".to_string(),
            Some("SESSION_NOT_FOUND".to_string()),
        );
    }
}
