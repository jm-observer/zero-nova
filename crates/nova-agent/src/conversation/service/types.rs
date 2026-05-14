use super::super::session::Session;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

pub(super) type SessionLoadResult = Option<Arc<Session>>;
pub(super) type LoadingWaiters = HashMap<String, Vec<oneshot::Sender<SessionLoadResult>>>;
