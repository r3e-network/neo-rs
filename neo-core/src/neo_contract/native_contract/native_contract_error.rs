use thiserror::Error;

#[derive(Error, Debug)]
pub enum NativeContractError {
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Insufficient balance")]
    InsufficientBalance,

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Unauthorized access")]
    Unauthorized,

    #[error("Contract execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Arithmetic error: {0}")]
    ArithmeticError(String),

    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}
