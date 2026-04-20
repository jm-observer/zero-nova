// Chat handler with turn routing (Phase 4)

use crate::gateway::control::{
    InteractionKind, InteractionResolver, PendingInteraction, ResolutionIntent, RiskLevel, TurnIntent,
};
use crate::gateway::handlers::system::send_general_error;
use crate::gateway::protocol::{
    ChatCompletePayload, ChatPayload, GatewayMessage, InteractionResolvedPayload, MessageEnvelope, SessionIdPayload,
};
use crate::gateway::router::AppState;
use crate::provider::LlmClient;
use log::error;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    // 检查 session_id 是否提供
    let Some(session_id_val) = payload.session_id.clone() else {
        send_general_error(
            &outbound_tx,
            &request_id,
            "session_id is required".to_string(),
            Some("INVALID_REQUEST".to_string()),
        );
        return;
    };

    // Retrieve the session.
    let Some(session) = state.sessions.get(&session_id_val).await else {
        send_general_error(
            &outbound_tx,
            &request_id,
            format!("Session {} not found", session_id_val),
            Some("SESSION_NOT_FOUND".to_string()),
        );
        return;
    };

    // Serialize access to the session's chat state.
    let _lock = session.chat_lock.lock().await;

    let intent = TurnIntent::ExecuteChat;

    match intent {
        TurnIntent::ResolvePendingInteraction => {
            handle_resolve_interaction::<C>(
                session.clone(),
                &payload.input,
                state.clone(),
                outbound_tx.clone(),
                request_id.clone(),
            )
            .await;
        }
        TurnIntent::AddressAgent { agent_id } => {
            handle_address_agent(
                session.clone(),
                agent_id,
                state.clone(),
                outbound_tx.clone(),
                request_id.clone(),
            )
            .await;
        }
        TurnIntent::ContinueWorkflow => {
            handle_continue_workflow::<C>(
                session.clone(),
                &payload.input,
                state.clone(),
                outbound_tx.clone(),
                request_id.clone(),
            )
            .await;
        }
        TurnIntent::StartNewTask { topic } => {
            handle_start_new_task::<C>(
                session.clone(),
                topic,
                &payload.input,
                state.clone(),
                outbound_tx.clone(),
                request_id.clone(),
            )
            .await;
        }
        TurnIntent::ExecuteChat => {
            // Normal chat flow.
            execute_chat_turn(session.clone(), &payload, state.clone(), outbound_tx, request_id).await;
        }
    }
}

/// Handle a chat stop request (cancellation).
pub async fn handle_chat_stop<C: LlmClient>(
    payload: SessionIdPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(session) = state.sessions.get(&payload.session_id).await {
        if let Some(token) = session.take_cancellation_token() {
            token.cancel();
        }
        let _ = outbound_tx.send(GatewayMessage::new(
            request_id,
            MessageEnvelope::ChatStopResponse(SessionIdPayload {
                session_id: payload.session_id,
            }),
        ));
    } else {
        send_general_error(
            &outbound_tx,
            &request_id,
            format!("Session {} not found", payload.session_id),
            Some("SESSION_NOT_FOUND".to_string()),
        );
    }
}

/// Resolve a pending interaction and send the result back to the client.
async fn handle_resolve_interaction<C: LlmClient>(
    session: Arc<crate::gateway::session::Session>,
    user_input: &str,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let mut control = session.control.write().unwrap();
    if let Some(pending) = control.pending_interaction.take() {
        let result = InteractionResolver::resolve(user_input, &pending);
        let result_str = match result.intent {
            ResolutionIntent::Approve => {
                // If it was a ConfirmSwitch, actually perform the switch
                if pending.kind == InteractionKind::Approve && pending.id.starts_with("switch:") {
                    let target_id = &pending.id[7..];
                    control.active_agent = target_id.to_string();
                }
                "approved"
            }
            ResolutionIntent::Reject => "rejected",
            ResolutionIntent::Select => "selected",
            ResolutionIntent::ProvideInput => "input",
            ResolutionIntent::Unclear => "unclear",
        };
        let payload = InteractionResolvedPayload {
            session_id: session.id.clone(),
            interaction_id: pending.id.clone(),
            result: result_str.to_string(),
        };
        let _ = outbound_tx.send(GatewayMessage::new(
            request_id.clone(),
            MessageEnvelope::InteractionResolved(payload),
        ));

        // If it was a switch, notify frontend
        if result_str == "approved" && pending.id.starts_with("switch:") {
            let target_id = &pending.id[7..];
            if let Some(agent) = state.agent_registry.get(target_id) {
                let _ = outbound_tx.send(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::AgentsSwitchResponse(crate::gateway::protocol::AgentsSwitchResponse {
                        agent: crate::gateway::protocol::Agent {
                            id: agent.id.clone(),
                            name: agent.display_name.clone(),
                            description: Some(agent.description.clone()),
                            ..Default::default()
                        },
                        messages: vec![],
                    }),
                ));
            }
        }
    }
}

/// Handle a workflow continuation step.
async fn handle_continue_workflow<C: LlmClient>(
    session: Arc<crate::gateway::session::Session>,
    user_input: &str,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let result = {
        let (wf_opt, event_tx) = {
            let control = session.control.read().unwrap();
            let wf_opt = control.workflow.clone();

            let (event_tx, mut event_rx) = mpsc::channel(100);
            let outbound_tx_clone = outbound_tx.clone();
            let request_id_clone = request_id.clone();
            let session_id_clone = session.id.clone();

            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    let gateway_msg =
                        crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
                    if outbound_tx_clone.send(gateway_msg).is_err() {
                        break;
                    }
                }
            });

            (wf_opt, event_tx)
        };

        if let Some(mut workflow) = wf_opt {
            let adv_res =
                crate::gateway::workflow::WorkflowEngine::advance(&mut workflow, user_input, &state.agent, event_tx)
                    .await;

            if adv_res.is_ok() {
                let mut control = session.control.write().unwrap();
                control.workflow = Some(workflow);
            }
            adv_res
        } else {
            return;
        }
    };

    match result {
        Ok(result) => {
            // Send messages
            for msg in result.messages {
                // Simple text message event for now
                let _ = outbound_tx.send(GatewayMessage::new_event(MessageEnvelope::ChatProgress(
                    crate::gateway::protocol::ProgressEvent {
                        kind: "token".to_string(),
                        token: Some(msg),
                        session_id: Some(session.id.clone()),
                        ..Default::default()
                    },
                )));
            }

            // If new interaction is requested, save it
            if let Some(pending) = result.new_pending {
                let mut control = session.control.write().unwrap();
                let payload = crate::gateway::protocol::InteractionRequestPayload {
                    session_id: session.id.clone(),
                    interaction_id: pending.id.clone(),
                    kind: match pending.kind {
                        InteractionKind::Approve => "approve".to_string(),
                        InteractionKind::Select => "select".to_string(),
                        InteractionKind::Input => "input".to_string(),
                    },
                    subject: pending.subject.clone(),
                    prompt: pending.prompt.clone(),
                    options: pending
                        .options
                        .iter()
                        .map(crate::gateway::protocol::InteractionOptionDTO::from)
                        .collect(),
                    risk_level: match pending.risk_level {
                        RiskLevel::Low => "low".to_string(),
                        RiskLevel::Medium => "medium".to_string(),
                        RiskLevel::High => "high".to_string(),
                    },
                };
                control.pending_interaction = Some(pending);
                let _ = outbound_tx.send(GatewayMessage::new_event(MessageEnvelope::InteractionRequest(payload)));
            }

            // Finalize turn
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ChatComplete(ChatCompletePayload {
                    session_id: session.id.clone(),
                    output: None,
                    usage: None,
                }),
            ));
        }
        Err(e) => {
            error!("Workflow advance error: {}", e);
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Workflow error: {}", e),
                Some("WORKFLOW_ERROR".to_string()),
            );
        }
    }
}

/// Handle a new task: create a workflow and run the first advance (GatherRequirements).
async fn handle_start_new_task<C: LlmClient>(
    session: Arc<crate::gateway::session::Session>,
    topic: String,
    user_input: &str,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    // Create a new workflow for this topic
    let mut workflow = crate::gateway::workflow::WorkflowState::new(topic);

    // Set up event forwarding
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session.id.clone();

    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // Run the first advance to kick off GatherRequirements
    let result =
        crate::gateway::workflow::WorkflowEngine::advance(&mut workflow, user_input, &state.agent, event_tx).await;

    // Persist workflow state regardless of advance result
    {
        let mut control = session.control.write().unwrap();
        control.workflow = Some(workflow);
    }

    match result {
        Ok(result) => {
            for msg in result.messages {
                let _ = outbound_tx.send(GatewayMessage::new_event(MessageEnvelope::ChatProgress(
                    crate::gateway::protocol::ProgressEvent {
                        kind: "token".to_string(),
                        token: Some(msg),
                        session_id: Some(session.id.clone()),
                        ..Default::default()
                    },
                )));
            }

            // If the first stage produces a pending interaction, save and send it
            if let Some(pending) = result.new_pending {
                let mut control = session.control.write().unwrap();
                let payload = crate::gateway::protocol::InteractionRequestPayload {
                    session_id: session.id.clone(),
                    interaction_id: pending.id.clone(),
                    kind: match pending.kind {
                        InteractionKind::Approve => "approve".to_string(),
                        InteractionKind::Select => "select".to_string(),
                        InteractionKind::Input => "input".to_string(),
                    },
                    subject: pending.subject.clone(),
                    prompt: pending.prompt.clone(),
                    options: pending
                        .options
                        .iter()
                        .map(crate::gateway::protocol::InteractionOptionDTO::from)
                        .collect(),
                    risk_level: match pending.risk_level {
                        RiskLevel::Low => "low".to_string(),
                        RiskLevel::Medium => "medium".to_string(),
                        RiskLevel::High => "high".to_string(),
                    },
                };
                control.pending_interaction = Some(pending);
                let _ = outbound_tx.send(GatewayMessage::new_event(MessageEnvelope::InteractionRequest(payload)));
            }

            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::ChatComplete(ChatCompletePayload {
                    session_id: session.id.clone(),
                    output: None,
                    usage: None,
                }),
            ));
        }
        Err(e) => {
            error!("Start new task error: {}", e);
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Failed to start task: {}", e),
                Some("WORKFLOW_ERROR".to_string()),
            );
        }
    }
}

/// Handle an explicit agent addressing request (triggering a switch confirmation).
async fn handle_address_agent<C: LlmClient>(
    session: Arc<crate::gateway::session::Session>,
    agent_id: String,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(agent) = state.agent_registry.get(&agent_id) {
        let mut control = session.control.write().unwrap();
        let interaction_id = format!("switch:{}", agent_id);

        let pending = PendingInteraction {
            id: interaction_id.clone(),
            kind: InteractionKind::Approve,
            subject: "Agent Switch".to_string(),
            prompt: format!("您是否要切换到 {}？", agent.display_name),
            options: vec![],
            risk_level: RiskLevel::Low,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            ttl_seconds: 300,
        };

        let payload = crate::gateway::protocol::InteractionRequestPayload {
            session_id: session.id.clone(),
            interaction_id: interaction_id.clone(),
            kind: "approve".to_string(),
            subject: pending.subject.clone(),
            prompt: pending.prompt.clone(),
            options: vec![],
            risk_level: "low".to_string(),
        };

        control.pending_interaction = Some(pending);
        let _ = outbound_tx.send(GatewayMessage::new(
            request_id,
            MessageEnvelope::InteractionRequest(payload),
        ));
    } else {
        send_general_error(
            &outbound_tx,
            &request_id,
            format!("Agent {} not found", agent_id),
            Some("AGENT_NOT_FOUND".to_string()),
        );
    }
}

/// Normal chat processing (Phase 3 unchanged).
async fn execute_chat_turn<C: LlmClient>(
    session: Arc<crate::gateway::session::Session>,
    payload: &ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.clone(),
        MessageEnvelope::ChatStart(SessionIdPayload {
            session_id: session.id.clone(),
        }),
    ));

    // 2. 写入 User Message (预写入防止失败丢失)
    session.append_user_message(&payload.input);

    // 3. 创建并注册 CancellationToken
    let token = CancellationToken::new();
    session.set_cancellation_token(token.clone());

    // 4. 事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session.id.clone();

    // 5. 转发任务
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // 6. 准备历史上下文（排除刚写入的用户消息）
    let history = session.get_history();
    let history_for_turn = &history[..history.len() - 1];

    // 7. 运行 turn
    match state
        .agent
        .run_turn(history_for_turn, &payload.input, event_tx, Some(token))
        .await
    {
        Ok(turn_result) => {
            session.append_assistant_messages(turn_result.messages);
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id.clone(),
                MessageEnvelope::ChatComplete(ChatCompletePayload {
                    session_id: session.id.clone(),
                    output: None,
                    usage: Some(turn_result.usage),
                }),
            ));
        }
        Err(e) => {
            error!("Agent execution error for session {}: {}", session.id, e);
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
