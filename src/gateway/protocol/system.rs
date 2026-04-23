use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WelcomePayload {
    pub require_auth: bool,
    pub setup_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub message: String,
    pub code: Option<String>,
}
