use anyhow::Result;

#[derive(Debug)]
pub enum TitleGenerationError {
    Retryable(anyhow::Error),
    NonRetryable(anyhow::Error),
}

impl std::fmt::Display for TitleGenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retryable(err) => write!(f, "retryable title generation error: {err}"),
            Self::NonRetryable(err) => write!(f, "non-retryable title generation error: {err}"),
        }
    }
}

impl std::error::Error for TitleGenerationError {}

#[async_trait::async_trait]
pub trait TitleGenerator: Send + Sync {
    async fn generate_title(&self, user_texts: &[String]) -> Result<String, TitleGenerationError>;
}

pub struct RuleBasedTitleGenerator;

#[async_trait::async_trait]
impl TitleGenerator for RuleBasedTitleGenerator {
    async fn generate_title(&self, user_texts: &[String]) -> Result<String, TitleGenerationError> {
        let joined = user_texts.join(" ");
        if joined.trim().is_empty() {
            return Err(TitleGenerationError::NonRetryable(anyhow::anyhow!(
                "user texts are empty"
            )));
        }
        Ok(joined)
    }
}
