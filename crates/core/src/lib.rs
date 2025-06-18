// Copyright (C) 2015-2025 The Neo Project.
//
// lib.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! # Neo Core
//!
//! Core functionality for the Neo blockchain, including basic types and utilities.

use serde::{Deserialize, Serialize};

// Module declarations
pub mod uint160;
pub mod uint256;
pub mod big_decimal;
pub mod transaction_type;
pub mod builders;
pub mod extensions;
pub mod hardfork;
pub mod events;
pub mod neo_system;
pub mod witness;
pub mod witness_scope;
pub mod witness_rule;
pub mod signer;
pub mod transaction;
pub mod shutdown;

// Re-exports
pub use uint160::UInt160;
pub use uint256::UInt256;
pub use big_decimal::BigDecimal;
pub use transaction_type::ContainsTransactionType;
pub use neo_system::NeoSystem;
pub use witness::Witness;
pub use witness_scope::WitnessScope;
pub use signer::Signer;
pub use witness_rule::{WitnessRule, WitnessCondition, WitnessRuleAction, WitnessConditionType};
pub use transaction::{Transaction, TransactionAttribute, TransactionAttributeType, OracleResponseCode, MAX_TRANSACTION_SIZE, MAX_TRANSACTION_ATTRIBUTES, HEADER_SIZE};
pub use shutdown::{ShutdownCoordinator, Shutdown, ShutdownStage, ShutdownEvent, ShutdownError, SignalHandler};

// Global singletons (matches C# Blockchain.Singleton and Store.Singleton exactly)
use std::sync::{Arc, RwLock};
use once_cell::sync::Lazy;

/// Global blockchain singleton (matches C# Blockchain.Singleton exactly)
pub static GLOBAL_BLOCKCHAIN: Lazy<Arc<RwLock<Option<transaction::blockchain::BlockchainSingleton>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// Global store singleton (matches C# Store.Singleton exactly)
pub static GLOBAL_STORE: Lazy<Arc<RwLock<Option<Box<dyn transaction::blockchain::PersistenceStore + Send + Sync>>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(None)));

// Export Error as an alias for CoreError for compatibility
pub use CoreError as Error;

// Add IVerifiable trait for compatibility
pub trait IVerifiable: std::any::Any {
    /// Verify the object
    fn verify(&self) -> bool;

    /// Get the hash of the object
    fn hash(&self) -> CoreResult<UInt256>;

    /// Get the hash data for signing
    fn get_hash_data(&self) -> Vec<u8>;

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

// Error handling
use thiserror::Error;

/// Core module errors
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("System error: {0}")]
    SystemError(String),

    #[error("Insufficient gas")]
    InsufficientGas,

    #[error("Cryptographic error: {0}")]
    CryptographicError(String),
}

/// Result type for core operations
pub type CoreResult<T> = Result<T, CoreError>;

// Add conversion from neo_io::Error to CoreError
impl From<neo_io::Error> for CoreError {
    fn from(error: neo_io::Error) -> Self {
        match error {
            neo_io::Error::EndOfStream => CoreError::InvalidData("Unexpected end of stream".to_string()),
            neo_io::Error::InvalidData(msg) => CoreError::InvalidData(msg),
            neo_io::Error::FormatException => CoreError::InvalidFormat("Format exception".to_string()),
            neo_io::Error::Deserialization(msg) => CoreError::SerializationError(msg),
            neo_io::Error::InvalidOperation(msg) => CoreError::InvalidOperation(msg),
            neo_io::Error::Io(msg) => CoreError::SystemError(msg),
            neo_io::Error::Serialization(msg) => CoreError::SerializationError(msg),
            neo_io::Error::InvalidFormat(msg) => CoreError::InvalidFormat(msg),
            neo_io::Error::BufferOverflow => CoreError::InvalidData("Buffer overflow".to_string()),
        }
    }
}
