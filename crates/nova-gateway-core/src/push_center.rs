use channel_core::{PeerId, ResponseSink};
use nova_protocol::GatewayMessage;
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct PushCenter {
    peers: RwLock<HashMap<PeerId, ResponseSink<GatewayMessage>>>,
    peer_sessions: RwLock<HashMap<PeerId, String>>,
    session_peers: RwLock<HashMap<String, HashSet<PeerId>>>,
}

impl PushCenter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_peer(&self, peer_id: PeerId, sink: ResponseSink<GatewayMessage>) {
        self.peers.write().await.insert(peer_id, sink);
    }

    pub async fn unregister_peer(&self, peer_id: &str) {
        self.peers.write().await.remove(peer_id);

        let previous_session = self.peer_sessions.write().await.remove(peer_id);
        if let Some(session_id) = previous_session {
            self.remove_peer_from_session(peer_id, &session_id).await;
        }
    }

    pub async fn subscribe_peer_to_session(&self, peer_id: &str, session_id: &str) {
        let previous_session = {
            let mut peer_sessions = self.peer_sessions.write().await;
            peer_sessions.insert(peer_id.to_string(), session_id.to_string())
        };

        if let Some(previous_session_id) = previous_session {
            if previous_session_id == session_id {
                return;
            }
            self.remove_peer_from_session(peer_id, &previous_session_id).await;
        }

        let mut session_peers = self.session_peers.write().await;
        session_peers
            .entry(session_id.to_string())
            .or_default()
            .insert(peer_id.to_string());
    }

    pub async fn broadcast_to_session(&self, session_id: &str, message: GatewayMessage) {
        self.broadcast_to_session_except(session_id, None, message).await;
    }

    pub async fn broadcast_to_session_except(
        &self,
        session_id: &str,
        excluded_peer_id: Option<&str>,
        message: GatewayMessage,
    ) {
        let peer_ids = {
            let session_peers = self.session_peers.read().await;
            session_peers
                .get(session_id)
                .map(|peers| peers.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };

        if peer_ids.is_empty() {
            return;
        }

        let sinks = {
            let peers = self.peers.read().await;
            peer_ids
                .iter()
                .filter(|peer_id| excluded_peer_id != Some(peer_id.as_str()))
                .filter_map(|peer_id| peers.get(peer_id).cloned().map(|sink| (peer_id.clone(), sink)))
                .collect::<Vec<_>>()
        };

        let mut stale_peers = Vec::new();
        for (peer_id, sink) in sinks {
            if sink.send_async(message.clone()).await.is_err() {
                stale_peers.push(peer_id);
            }
        }

        for peer_id in stale_peers {
            self.unregister_peer(&peer_id).await;
        }
    }

    async fn remove_peer_from_session(&self, peer_id: &str, session_id: &str) {
        let mut session_peers = self.session_peers.write().await;
        let should_remove_session = if let Some(peers) = session_peers.get_mut(session_id) {
            peers.remove(peer_id);
            peers.is_empty()
        } else {
            false
        };

        if should_remove_session {
            session_peers.remove(session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PushCenter;
    use channel_core::ResponseSink;
    use nova_protocol::{GatewayMessage, MessageEnvelope, WelcomePayload};
    use tokio::sync::mpsc;

    fn welcome_event() -> GatewayMessage {
        GatewayMessage::new_event(MessageEnvelope::Welcome(WelcomePayload {
            require_auth: false,
            setup_required: false,
        }))
    }

    #[tokio::test]
    async fn broadcasts_only_to_subscribed_session() {
        let push_center = PushCenter::new();
        let (tx_a, mut rx_a) = mpsc::channel(4);
        let (tx_b, mut rx_b) = mpsc::channel(4);

        push_center
            .register_peer("peer-a".to_string(), ResponseSink::new(tx_a))
            .await;
        push_center
            .register_peer("peer-b".to_string(), ResponseSink::new(tx_b))
            .await;
        push_center.subscribe_peer_to_session("peer-a", "session-a").await;
        push_center.subscribe_peer_to_session("peer-b", "session-b").await;

        push_center.broadcast_to_session("session-a", welcome_event()).await;

        assert!(rx_a.recv().await.is_some());
        assert!(rx_b.try_recv().is_err());
    }

    #[tokio::test]
    async fn resubscribe_replaces_previous_session_binding() {
        let push_center = PushCenter::new();
        let (tx, mut rx) = mpsc::channel(4);

        push_center
            .register_peer("peer-a".to_string(), ResponseSink::new(tx))
            .await;
        push_center.subscribe_peer_to_session("peer-a", "session-a").await;
        push_center.subscribe_peer_to_session("peer-a", "session-b").await;

        push_center.broadcast_to_session("session-a", welcome_event()).await;
        assert!(rx.try_recv().is_err());

        push_center.broadcast_to_session("session-b", welcome_event()).await;
        assert!(rx.recv().await.is_some());
    }
}
