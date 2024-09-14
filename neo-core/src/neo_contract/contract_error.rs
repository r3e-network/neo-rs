use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    #[error("Invalid operation")]
    InvalidOperation,
    #[error("Invalid argument")]
    InvalidArgument,
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Contract not found")]
    ContractNotFound,
    #[error("Method not found")]
    MethodNotFound,
    #[error("Execution reverted")]
    ExecutionReverted,
    #[error("Storage error")]
    StorageError,
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,
    #[error("Unsupported feature")]
    UnsupportedFeature,
    #[error("Gas limit exceeded")]
    GasLimitExceeded,
    #[error("Invalid state")]
    InvalidState,
    #[error("Unknown error")]
    UnknownError,
}
