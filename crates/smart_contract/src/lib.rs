//! Neo Smart Contract Engine
//!
//! This crate provides the smart contract execution engine for the Neo blockchain,
//! including contract deployment, invocation, and native contract implementations.

// Re-export from other crates for convenience
pub use neo_core::{
    UInt160, UInt256, Transaction, Witness, Signer, IVerifiable,
    WitnessScope, WitnessCondition, TransactionAttributeType,
};
pub use neo_cryptography::ECPoint;
pub use neo_vm::{ApplicationEngine, TriggerType};
// Import types from ledger that are needed
pub use neo_ledger::blockchain::state::{ContractParameterType, PermissionContract};

pub mod application_engine;
pub mod contract_state;
pub mod deployment;
pub mod events;
pub mod interop;
pub mod manifest;
pub mod native;
pub mod performance;
pub mod storage;
pub mod validation;

use thiserror::Error;

/// Smart contract error types
#[derive(Debug, Error)]
pub enum Error {
    #[error("VM error: {0}")]
    VM(#[from] neo_vm::Error),
    #[error("Core error: {0}")]
    Core(String),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Invalid contract: {0}")]
    InvalidContract(String),
    #[error("Invalid witness: {0}")]
    InvalidWitness(String),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Gas limit exceeded")]
    GasLimitExceeded,
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Storage not found: {0}")]
    StorageNotFound(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Event error: {0}")]
    Event(#[from] EventError),
    #[error("Native contract error: {0}")]
    NativeContractError(String),
    #[error("Execution halted: {0}")]
    ExecutionHalted(String),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("VM execution error: {0}")]
    VmError(String),
    #[error("Contract not found: {0}")]
    ContractNotFound(String),
    #[error("Insufficient gas: {0}")]
    InsufficientGas(String),
    #[error("Interop service error: {0}")]
    InteropServiceError(String),
    #[error("Insufficient funds: {0}")]
    InsufficientFunds(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("Neo token error: {0}")]
    NeoToken(#[from] NeoTokenError),
}

// Add From trait implementations for error type conversions
impl From<neo_io::Error> for Error {
    fn from(err: neo_io::Error) -> Self {
        match err {
            neo_io::Error::EndOfStream => Error::SerializationError("Unexpected end of stream".to_string()),
            neo_io::Error::InvalidData(msg) => Error::SerializationError(msg),
            neo_io::Error::FormatException => Error::SerializationError("Format exception".to_string()),
            neo_io::Error::Deserialization(msg) => Error::SerializationError(msg),
            neo_io::Error::InvalidOperation(msg) => Error::InvalidOperation(msg),
            neo_io::Error::Io(msg) => Error::IO(std::io::Error::new(std::io::ErrorKind::Other, msg)),
            neo_io::Error::Serialization(msg) => Error::SerializationError(msg),
            neo_io::Error::InvalidFormat(msg) => Error::SerializationError(msg),
            neo_io::Error::BufferOverflow => Error::SerializationError("Buffer overflow".to_string()),
        }
    }
}

impl From<neo_core::CoreError> for Error {
    fn from(err: neo_core::CoreError) -> Self {
        match err {
            neo_core::CoreError::InvalidFormat(msg) => Error::SerializationError(msg),
            neo_core::CoreError::InvalidData(msg) => Error::SerializationError(msg),
            neo_core::CoreError::IoError(io_err) => Error::IO(io_err),
            neo_core::CoreError::SerializationError(msg) => Error::SerializationError(msg),
            neo_core::CoreError::InvalidOperation(msg) => Error::InvalidOperation(msg),
            neo_core::CoreError::SystemError(msg) => Error::Core(msg),
            neo_core::CoreError::InsufficientGas => Error::InsufficientGas("Insufficient gas".to_string()),
            neo_core::CoreError::CryptographicError(msg) => Error::InvalidSignature(msg),
        }
    }
}

/// Event-specific errors
#[derive(Debug, Error)]
pub enum EventError {
    #[error("Invalid delay: {0}")]
    InvalidDelay(String),
    #[error("HTTP error: {0}")]
    HttpError(String),
    #[error("WebSocket error: {0}")]
    WebSocketError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// NeoToken-specific errors  
#[derive(Debug, Error)]
pub enum NeoTokenError {
    #[error("Invalid candidate: {0}")]
    InvalidCandidate(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
}

pub type Result<T> = std::result::Result<T, Error>;
