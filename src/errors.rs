use thiserror::Error;

#[derive(Error, Debug)]
pub enum CedarError {
    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Google Docs API error: {0}")]
    GoogleDocs(String),

    #[error("Google Drive API error: {0}")]
    GoogleDrive(String),

    #[error("Document sync error: {0}")]
    Sync(String),

    #[error("Diff calculation error: {0}")]
    Diff(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("OAuth2 error: {0}")]
    OAuth2(String),

    #[error("Index out of bounds: {0}")]
    IndexOutOfBounds(String),

    #[error("Conflict detected: {0}")]
    Conflict(String),

    #[error("Rate limit exceeded: {retry_after_seconds} seconds")]
    RateLimit { retry_after_seconds: u64 },
}

pub type Result<T> = std::result::Result<T, CedarError>;
