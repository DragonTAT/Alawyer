use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum CoreError {
    #[error("Config error: {0}")]
    Config(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("Tool error: {0}")]
    Tool(String),
    #[error("Safety violation: {0}")]
    Safety(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
