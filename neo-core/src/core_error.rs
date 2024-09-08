use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Contract error: {0}")]
    ContractError(String),

    #[error("Insufficient GAS: required {0}, available {1}")]
    InsufficientGas(u64, u64),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl CoreError {
    pub fn new_invalid_operation(msg: &str) -> Self {
        CoreError::InvalidOperation(msg.to_string())
    }

    pub fn new_storage_error(msg: &str) -> Self {
        CoreError::StorageError(msg.to_string())
    }

    pub fn new_execution_error(msg: &str) -> Self {
        CoreError::ExecutionError(msg.to_string())
    }

    pub fn new_contract_error(msg: &str) -> Self {
        CoreError::ContractError(msg.to_string())
    }

    pub fn new_insufficient_gas(required: u64, available: u64) -> Self {
        CoreError::InsufficientGas(required, available)
    }

    pub fn new_invalid_transaction(msg: &str) -> Self {
        CoreError::InvalidTransaction(msg.to_string())
    }

    pub fn new_serialization_error(msg: &str) -> Self {
        CoreError::SerializationError(msg.to_string())
    }

    pub fn new_deserialization_error(msg: &str) -> Self {
        CoreError::DeserializationError(msg.to_string())
    }

    pub fn new_crypto_error(msg: &str) -> Self {
        CoreError::CryptoError(msg.to_string())
    }

    pub fn new_network_error(msg: &str) -> Self {
        CoreError::NetworkError(msg.to_string())
    }

    pub fn new_unsupported_feature(msg: &str) -> Self {
        CoreError::UnsupportedFeature(msg.to_string())
    }

    pub fn new_unknown(msg: &str) -> Self {
        CoreError::Unknown(msg.to_string())
    }
}
