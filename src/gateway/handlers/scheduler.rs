use crate::gateway::protocol::{GatewayMessage, MessageEnvelope};
use serde_json::json;
use tokio::sync::mpsc;

/// 处理获取定时任务列表请求 (目前返回空列表作为 Stub)
pub async fn handle_scheduler_list(outbound_tx: mpsc::UnboundedSender<GatewayMessage>, msg_id: String) {
    let _ = outbound_tx.send(GatewayMessage::new(
        msg_id,
        MessageEnvelope::SchedulerListResponse(json!({ "tasks": [] })),
    ));
}
