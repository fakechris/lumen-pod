use thiserror::Error;

#[derive(Debug, Error)]
pub enum PodError {
    #[error("episode not found: {0}")]
    NotFound(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type PodResult<T> = Result<T, PodError>;
