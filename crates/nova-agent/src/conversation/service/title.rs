use super::helpers::normalize_generated_title;
use super::{
    SessionService, TITLE_GENERATION_TIMEOUT_MS, TITLE_MAX_ATTEMPTS, TITLE_MIN_TOTAL_CHARS,
    TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT, TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT,
};
use crate::conversation::control::{TitleSource, TitleStatus};
use crate::conversation::session::Session;
use crate::conversation::title_generator::TitleGenerationError;
use crate::message::{ContentBlock, Role};
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

impl SessionService {
    pub(super) async fn maybe_schedule_title_generation(&self, session: Arc<Session>) -> Result<()> {
        let (can_schedule, user_messages_count, user_texts) = {
            let mut title_state = session.title_state.write().await;
            let history = session.history.read().await;

            if title_state.source != TitleSource::Default
                || title_state.status == TitleStatus::Pending
                || title_state.attempt_count >= TITLE_MAX_ATTEMPTS
            {
                return Ok(());
            }

            let user_texts: Vec<String> = history
                .iter()
                .filter(|m| m.role == Role::User)
                .flat_map(|m| m.content.iter())
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.trim()),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            let user_messages_count = user_texts.len();
            let total_chars = user_texts.iter().map(|text| text.chars().count()).sum::<usize>();
            let min_messages = if title_state.attempt_count == 0 {
                TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT
            } else {
                TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT
            };
            if user_messages_count < min_messages || total_chars < TITLE_MIN_TOTAL_CHARS {
                return Ok(());
            }

            title_state.set_pending(user_messages_count);
            (true, user_messages_count, user_texts)
        };

        if !can_schedule {
            return Ok(());
        }
        self.persist_runtime_control(&session.id, &session).await?;

        let this = self.clone();
        tokio::spawn(async move {
            if let Err(err) = this
                .run_title_generation(session.clone(), user_messages_count, user_texts)
                .await
            {
                log::error!(
                    "Session title generation task failed: session_id={}, err={}",
                    session.id,
                    err
                );
            }
        });

        Ok(())
    }

    pub(super) async fn run_title_generation(
        &self,
        session: Arc<Session>,
        user_message_count: usize,
        user_texts: Vec<String>,
    ) -> Result<()> {
        let generation_result = timeout(
            Duration::from_millis(TITLE_GENERATION_TIMEOUT_MS),
            self.title_generator.generate_title(&user_texts),
        )
        .await;

        let generated = match generation_result {
            Ok(Ok(title)) => Ok(title),
            Ok(Err(TitleGenerationError::Retryable(err))) => Err(format!("retryable: {err}")),
            Ok(Err(TitleGenerationError::NonRetryable(err))) => Err(format!("non_retryable: {err}")),
            Err(_) => Err(format!("retryable: timeout after {}ms", TITLE_GENERATION_TIMEOUT_MS)),
        };

        let mut should_update_title = false;
        let mut normalized = String::new();
        {
            let mut title_state = session.title_state.write().await;
            match generated {
                Ok(title) => {
                    normalized = normalize_generated_title(&title);
                    if normalized.is_empty() {
                        title_state
                            .set_failed("non_retryable: generated title is empty after normalization".to_string());
                    } else {
                        should_update_title = true;
                    }
                }
                Err(err_msg) => title_state.set_failed(err_msg),
            }
            if should_update_title {
                title_state.set_succeeded();
                title_state.based_on_user_message_count = user_message_count;
            }
            if title_state.status == TitleStatus::Pending {
                title_state.set_failed("retryable: unexpected pending state".to_string());
            }
        }

        if should_update_title {
            session.set_name(normalized).await;
        }

        self.persist_session_control(&session).await?;
        Ok(())
    }
}
