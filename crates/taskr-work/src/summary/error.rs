use crate::store::StoreError;

#[derive(Debug)]
pub enum SummaryError {
    NotFound,
    NotEnrolled,
    Erased,
    NotReady,
    Conflict,
    Invalid(String),
    NotConfigured,
    Unavailable(String),
}

impl From<StoreError> for SummaryError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotEnrolled => Self::NotEnrolled,
            StoreError::Erased => Self::Erased,
            StoreError::Conflict => Self::Conflict,
            StoreError::Unavailable(why) => Self::Unavailable(why),
        }
    }
}
impl From<sqlx::Error> for SummaryError {
    fn from(error: sqlx::Error) -> Self { Self::Unavailable(error.to_string()) }
}
impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("No such work or generation."),
            Self::NotEnrolled => f.write_str("Enroll in audio work first."),
            Self::Erased => f.write_str("This account is unavailable due to account erasure."),
            Self::NotReady => f.write_str("An owned completed transcript is required."),
            Self::Conflict => f.write_str("The request id or active generation conflicts."),
            Self::Invalid(why) | Self::Unavailable(why) => f.write_str(why),
            Self::NotConfigured => f.write_str("Audio summary generation is not configured."),
        }
    }
}
impl std::error::Error for SummaryError {}
