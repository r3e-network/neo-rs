use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Timeout occurred")]
    Timeout,

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Network is unreachable")]
    NetworkUnreachable,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}
