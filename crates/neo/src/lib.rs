// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! # Neo Core
//!
//! Core functionality for the Neo blockchain implementation.
//!
//! This crate provides the fundamental types, traits, and utilities that form
//! the foundation of the Neo blockchain protocol. It includes essential components
//! for blocks, transactions, cryptographic operations, and system management.
//!
//! ## Features
//!
//! - **Type System**: Core types like `UInt160`, `UInt256`, and `BigDecimal`
//! - **Block Structure**: Block and block header implementations
//! - **Transaction System**: Transaction types, attributes, and validation
//! - **Witness System**: Witness, signer, and witness rule implementations
//! - **Error Handling**: Comprehensive error types and result handling
//! - **Monitoring**: System metrics and performance monitoring
//! - **Shutdown Coordination**: Graceful shutdown mechanisms
//!
//! ## Example
//!
//! ```rust,no_run
//! use crate::neo_core::{UInt256, Transaction, Block};
//!
//! // Create a new transaction hash
//! let hash = UInt256::zero();
//!
//! // Work with transactions and blocks
//! // let transaction = Transaction::new();
//! // let block = Block::new();
//! ```
//!
//! ## Architecture
//!
//! The core crate is organized into several key modules:
//!
//! - **Basic Types**: `uint160`, `uint256`, `big_decimal` - Fundamental data types
//! - **Blockchain**: `block`, `transaction`, `witness` - Blockchain primitives
//! - **System**: `neo_system`, `shutdown`, `monitoring` - System management
//! - **Utilities**: `error_handling`, `safe_operations` - Helper functionality

//#![warn(missing_docs)]
//#![warn(rustdoc::missing_crate_level_docs)]
extern crate self as neo_core;

// Module declarations with documentation
// Advanced metrics moved to neo-monitoring crate
/// Big decimal arithmetic implementation
pub mod big_decimal;
// Block moved to ledger module
/// Builder pattern implementations for complex types
// Removed builders - not in C# structure
/// System-wide constants
pub mod constants;
/// Contains transaction type enumeration
pub mod contains_transaction_type;
/// Core error types and error handling
pub mod error;
// Advanced error handling utilities moved to extensions
// Event system moved to i_event_handlers
// Extensions moved to neo-extensions crate
/// Compression utilities (matches Neo.Extensions compression helpers)
pub mod compression;
/// Cryptographic utilities using external crates
pub mod cryptography;
/// Hard fork management
pub mod hardfork;
/// Protocol settings configuration (matches C# ProtocolSettings)
pub mod protocol_settings;
// System metrics moved to neo-monitoring crate
// Monitoring moved to neo-monitoring crate
/// Neo system management
pub mod neo_system;
// Transaction signer moved to sign module
// Transaction structures moved to ledger module
// Transaction type definitions moved to ledger module
// Transaction validation moved to ledger module
/// 160-bit unsigned integer implementation
pub mod uint160;
/// 256-bit unsigned integer implementation
pub mod uint256;
/// Witness verification system
pub mod witness;
/// Witness rule evaluation
pub mod witness_rule;
/// Witness scope definitions
pub mod witness_scope;

// === C# Neo Main Project Structure ===
// SmartContract module (matches C# Neo.SmartContract)
pub mod smart_contract;
// Ledger module (matches C# Neo.Ledger)
pub mod ledger;
// Network module (matches C# Neo.Network)
pub mod network;
// Persistence module (matches C# Neo.Persistence)
pub mod persistence;
// Wallets module (matches C# Neo.Wallets)
pub mod wallets;
// Sign module (matches C# Neo.Sign)
pub mod sign;
// IEventHandlers module (matches C# Neo.IEventHandlers)
pub mod i_event_handlers;
// Extensions module (matches C# Neo.Extensions)
pub mod extensions;
// IO module (matches C# Neo.IO)
pub mod io;
// Plugins module (matches C# Neo.Plugins)
pub mod plugins;
// Time provider module (matches C# Neo.TimeProvider)
pub mod time_provider;

// Re-exports for convenient access
pub use big_decimal::BigDecimal;
pub use contains_transaction_type::ContainsTransactionType;
pub use cryptography::crypto_utils::{ECCurve, ECPoint};
pub use error::{CoreError, CoreResult, Result};
pub use hardfork::Hardfork;
pub use neo_system::NeoSystem;
pub use network::p2p::payloads::{
    Transaction, TransactionAttribute, TransactionAttributeType, HEADER_SIZE,
    MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE,
};
pub use time_provider::TimeProvider;
pub use uint160::UInt160;
pub use uint256::UInt256;
pub use witness::Witness;
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
pub use witness_scope::WitnessScope;

/// Compatibility re-export ensuring modules translated from C# continue to compile.
pub mod system {
    pub use crate::neo_system::*;
}

use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};

// NOTE: Global blockchain and store singletons will be implemented
// when the proper types are available in their respective modules.
// The C# implementation has:
// - Blockchain.Singleton
// - Store.Singleton
// These will be properly typed once all dependencies are in place.

/// Trait for verifiable blockchain objects.
///
/// This trait defines the interface for objects that can be cryptographically
/// verified, such as blocks and transactions. It provides methods for verification,
/// hashing, and type conversion.
///
/// # Implementation Requirements
///
/// Types implementing this trait must provide:
/// - Verification logic
/// - Hash computation
/// - Serialization for signing
/// - Type erasure support via `Any`
pub trait IVerifiable: std::any::Any + Send + Sync {
    /// Verifies the cryptographic validity of the object.
    ///
    /// # Returns
    ///
    /// `true` if the object is valid, `false` otherwise.
    fn verify(&self) -> bool;

    /// Computes the hash of the object.
    ///
    /// # Returns
    ///
    /// A `CoreResult` containing the computed hash or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if hash computation fails.
    fn hash(&self) -> CoreResult<UInt256>;

    /// Gets the serialized data used for hash computation.
    ///
    /// This method returns the byte representation that will be hashed
    /// to produce the object's identifier.
    ///
    /// # Returns
    ///
    /// A vector of bytes representing the hashable data.
    fn get_hash_data(&self) -> Vec<u8>;

    /// Returns a reference to self as `Any` for downcasting.
    ///
    /// This enables runtime type checking and downcasting to concrete types.
    ///
    /// # Returns
    ///
    /// A reference to self as a trait object.
    fn as_any(&self) -> &dyn std::any::Any;
}

pub mod neo_config {
    pub use crate::constants::*;
}

pub use network::p2p::messages::{
    MessageHeader as NetworkMessageHeader, NetworkMessage, ProtocolMessage,
};
pub use network::{NetworkError, NetworkResult};

pub mod neo_io {
    pub use crate::io::Serializable as ISerializable;
    pub use crate::io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

    pub mod serializable {
        use super::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

        pub mod helper {
            use super::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

            /// Returns the number of bytes required to encode a variable-length integer.
            pub fn get_var_size(value: u64) -> usize {
                if value < 0xFD {
                    1
                } else if value <= 0xFFFF {
                    3
                } else if value <= 0xFFFF_FFFF {
                    5
                } else {
                    9
                }
            }

            /// Serializes an array of `Serializable` items with a length prefix.
            pub fn serialize_array<T>(items: &[T], writer: &mut BinaryWriter) -> IoResult<()>
            where
                T: Serializable,
            {
                writer.write_var_int(items.len() as u64)?;
                for item in items {
                    item.serialize(writer)?;
                }
                Ok(())
            }

            /// Deserializes an array of `Serializable` items with an upper bound check.
            pub fn deserialize_array<T>(reader: &mut MemoryReader, max: usize) -> IoResult<Vec<T>>
            where
                T: Serializable,
            {
                let count = reader.read_var_int(max as u64)? as usize;
                if count > max {
                    return Err(IoError::invalid_data("Array length exceeds maximum"));
                }

                let mut result = Vec::with_capacity(count);
                for _ in 0..count {
                    result.push(T::deserialize(reader)?);
                }
                Ok(result)
            }
        }
    }

    pub use serializable::helper;

    /// Extension helpers for working with `Serializable` values.
    pub trait SerializableExt {
        /// Serializes the value into a freshly allocated byte vector.
        fn to_array(&self) -> IoResult<Vec<u8>>;
    }

    impl<T> SerializableExt for T
    where
        T: Serializable,
    {
        fn to_array(&self) -> IoResult<Vec<u8>> {
            let mut writer = BinaryWriter::new();
            self.serialize(&mut writer)?;
            Ok(writer.into_bytes())
        }
    }
}

pub mod neo_crypto {
    use crate::cryptography::crypto_utils::NeoHash;

    /// Computes SHA-256 hash (matches C# Neo.Cryptography.Crypto.Sha256).
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        NeoHash::sha256(data)
    }

    /// Computes Hash256 (double SHA-256) (matches C# Neo.Cryptography.Crypto.Hash256).
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        NeoHash::hash256(data)
    }

    /// Computes RIPEMD-160 hash (matches C# Neo.Cryptography.Crypto.RIPEMD160).
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        NeoHash::ripemd160(data)
    }
}

pub mod neo_cryptography {
    use crate::cryptography::crypto_utils::NeoHash;

    /// Generic cryptography error type (matches C# Neo.Cryptography.CryptographyException semantics).
    #[derive(Debug, Clone)]
    pub struct Error(pub String);

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for Error {}

    impl From<String> for Error {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    impl From<&str> for Error {
        fn from(value: &str) -> Self {
            Self(value.to_string())
        }
    }

    pub mod hash {
        use crate::cryptography::crypto_utils::NeoHash;

        pub fn sha256(data: &[u8]) -> [u8; 32] {
            NeoHash::sha256(data)
        }

        pub fn hash256(data: &[u8]) -> [u8; 32] {
            NeoHash::hash256(data)
        }

        pub fn ripemd160(data: &[u8]) -> [u8; 20] {
            NeoHash::ripemd160(data)
        }
    }

    pub mod ecc {
        use super::{ECCurve, ECPoint, Error};

        #[derive(Debug, Clone)]
        pub struct ECCError(pub String);

        impl std::fmt::Display for ECCError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::error::Error for ECCError {}

        impl From<String> for ECCError {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        pub fn decode_point(data: &[u8], curve: ECCurve) -> Result<ECPoint, ECCError> {
            ECPoint::decode(data, curve).map_err(ECCError::from)
        }

        pub fn decode_compressed_point(data: &[u8]) -> Result<ECPoint, ECCError> {
            ECPoint::decode_compressed(data).map_err(ECCError::from)
        }
    }

    pub use crate::cryptography::crypto_utils::{ECCurve, ECPoint};

    /// Simplified Merkle tree implementation for compatibility tests.
    pub struct MerkleTree;

    impl MerkleTree {
        pub fn compute_root(hashes: &[Vec<u8>]) -> Option<Vec<u8>> {
            if hashes.is_empty() {
                return None;
            }

            let mut current: Vec<Vec<u8>> = hashes.to_vec();
            while current.len() > 1 {
                let mut next = Vec::with_capacity((current.len() + 1) / 2);
                for chunk in current.chunks(2) {
                    let combined = if chunk.len() == 2 {
                        [chunk[0].as_slice(), chunk[1].as_slice()].concat()
                    } else {
                        [chunk[0].as_slice(), chunk[0].as_slice()].concat()
                    };
                    next.push(NeoHash::hash256(&combined).to_vec());
                }
                current = next;
            }

            current.into_iter().next()
        }
    }
}

pub mod neo_ledger {
    pub use crate::ledger::{
        block::Block, block_header::BlockHeader, blockchain::Blockchain,
        blockchain_application_executed::ApplicationExecuted, header_cache::HeaderCache,
        memory_pool::MemoryPool, verify_result::VerifyResult,
    };
}
