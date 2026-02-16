// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Core
//!
//! Core blockchain protocol implementation for Neo N3.
//!
//! This crate provides the fundamental types, traits, and utilities that form
//! the backbone of the Neo blockchain protocol. It implements the core logic
//! for blocks, transactions, smart contracts, and system management.
//!
//! ## Architecture
//!
//! The crate is organized into modules that mirror the C# Neo project structure:
//!
//! | Module | C# Equivalent | Purpose |
//! |--------|---------------|---------|
//! | [`ledger`] | `Neo.Ledger` | Blocks, transactions, blockchain state |
//! | [`smart_contract`] | `Neo.SmartContract` | Contract execution, native contracts |
//! | [`wallets`] | `Neo.Wallets` | Wallet management, key handling |
//! | [`network`] | `Neo.Network` | P2P networking, message handling |
//! | [`persistence`] | `Neo.Persistence` | Data storage, caching |
//! | [`services`] | - | Service trait definitions |
//!
//! ## Layer Position
//!
//! This crate is part of **Layer 1 (Core)** in the neo-rs architecture:
//!
//! ```text
//! Layer 2 (Service): neo-chain, neo-mempool
//!            │
//!            ▼
//! Layer 1 (Core):   neo-core ◄── YOU ARE HERE
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives, neo-crypto, neo-storage
//! ```
//!
//! ## Dependencies
//!
//! This crate depends only on Layer 0 (Foundation) crates:
//! - [`neo_primitives`]: Core types (`UInt160`, `UInt256`)
//! - [`neo_crypto`]: Cryptographic operations
//! - [`neo_storage`]: Storage traits
//! - [`neo_io`]: I/O operations
//! - [`neo_json`]: JSON handling
//!
//! ## Features
//!
//! - `runtime`: Enables actor-based runtime components (`NeoSystem`, actors)
//! - `monitoring`: Enables metrics collection and monitoring
//!
//! ## Example
//!
//! ```rust,no_run
//! use neo_core::{Block, Transaction, ProtocolSettings};
//! use neo_primitives::UInt256;
//!
//! // Load protocol settings
//! let settings = ProtocolSettings::default();
//!
//! // Create a transaction
//! let tx = Transaction::default();
//!
//! // Work with block hashes
//! let hash = UInt256::zero();
//! ```
//!
//! ## IVerifiable Trait
//!
//! The [`IVerifiable`] trait is central to blockchain validation. It is implemented
//! by types that can be cryptographically verified, such as blocks and transactions:
//!
//! ```rust,no_run
//! use neo_core::IVerifiable;
//!
//! fn verify_container<T: IVerifiable>(container: &T) -> bool {
//!     container.verify()
//! }
//! ```

// Documentation warnings deferred — tracked for incremental doc coverage
#![allow(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Self-reference for macro exports
extern crate self as neo_core;

// ============================================================================
// Declarative Macros (must be declared before other modules)
// ============================================================================

#[macro_use]
pub mod macros;

// ============================================================================
// Foundation Modules
// ============================================================================

/// Big decimal arithmetic for precise financial calculations.
///
/// Provides `BigDecimal` for handling NEO/GAS values with proper decimal precision.
pub mod big_decimal;

/// Builder pattern implementations for complex types.
///
/// Contains builders for `Transaction`, `Signer`, `Witness`, and witness conditions.
pub mod builders;

/// System-wide protocol constants.
///
/// Network magic numbers, port defaults, fee constants, and size limits.
pub mod constants;

/// Transaction containment type enumeration.
/// Transaction type containment checking.
///
/// Provides utilities for checking if a transaction contains specific types.
pub mod contains_transaction_type;

/// Core error types and error handling utilities.
/// Core error types and error handling utilities.
///
/// This module provides comprehensive error handling for Neo core operations,
/// including serialization errors, validation failures, and system errors.
pub mod error;

/// Plugin-style exception handling policies.
/// Plugin-style exception handling policies.
///
/// Defines how unhandled exceptions should be processed by the system.
pub mod unhandled_exception_policy;

/// Compression utilities (LZ4, gzip).
/// Compression utilities for data serialization.
///
/// Supports LZ4 and gzip compression algorithms for efficient data storage.
pub mod compression;

/// Cryptographic helper utilities.
/// Cryptographic helper utilities.
///
/// Provides ECC operations, signature verification, and key derivation helpers.
pub mod cryptography;

/// Blockchain hardfork management.
///
/// Tracks protocol upgrades (Aspidochelone, Basilisk, etc.).
pub mod hardfork;

/// Commonly used type re-exports.
pub mod prelude;

/// Protocol settings and network configuration.
///
/// Matches C# `ProtocolSettings` class.
pub mod protocol_settings;

/// Witness verification system.
///
/// Handles script verification for transactions.
pub mod witness;

/// Witness rule evaluation for conditional verification.
pub mod witness_rule;

// ============================================================================
// Optional Features
// ============================================================================

/// Monitoring and metrics (requires `monitoring` feature).
#[cfg(feature = "monitoring")]
pub mod monitoring;

/// Telemetry infrastructure for logging and tracing.
pub mod telemetry;

// ============================================================================
// C# Neo Project Modules
// ============================================================================

/// Smart contract execution and native contracts.
///
/// Matches C# `Neo.SmartContract` namespace.
pub mod smart_contract;

/// Ledger management: blocks, transactions, headers.
///
/// Matches C# `Neo.Ledger` namespace.
pub mod ledger;

/// Network layer: P2P messages, payloads, protocols.
///
/// Matches C# `Neo.Network` namespace.
pub mod network;

/// Data persistence: storage, caching, snapshots.
///
/// Matches C# `Neo.Persistence` namespace.
pub mod persistence;

/// Wallet management and key operations.
///
/// Matches C# `Neo.Wallets` namespace.
pub mod wallets;

/// Transaction signing and signature handling.
///
/// Matches C# `Neo.Sign` namespace.
pub mod sign;

/// Event handler interfaces.
///
/// Matches C# `Neo.IEventHandlers` namespace.
pub mod i_event_handlers;

/// Extension methods and utilities.
///
/// Matches C# `Neo.Extensions` namespace.
pub mod extensions;

/// Event system for blockchain notifications.
///
/// Matches C# `Neo.Events` namespace.
pub mod events;

/// I/O abstractions and helpers.
///
/// Matches C# `Neo.IO` namespace.
pub mod io;

/// RPC models and utilities.
pub mod rpc;

/// Time provider abstraction for testability.
pub mod time_provider;

/// Block and transaction validation utilities.
///
/// Provides comprehensive security checks for blocks including:
/// - Size limits (4 MB max)
/// - Transaction count limits (65535 max)
/// - Timestamp bounds (within 15 minutes of current time)
/// - Merkle root verification
/// - Witness script validation
pub mod validation;

/// State service for world state management.
pub mod state_service;

/// Service trait definitions for dependency injection.
pub mod services;

// ============================================================================
// Merged Modules (formerly separate crates)
// ============================================================================

/// Transaction mempool for pending transactions.
///
/// Provides lightweight mempool implementation for transaction management.
/// For full C# parity, use `ledger::MemoryPool` instead.
pub mod mempool;

/// Configuration management for Neo N3 blockchain node.
///
/// Provides node settings, protocol parameters, and network configuration.
pub mod config;

/// Blockchain state machine and chain management.
///
/// Provides chain state, block indexing, fork choice, and validation.
pub mod chain;

/// World state abstraction for Neo N3 blockchain.
///
/// Provides account state, contract storage, snapshots, and state trie.
pub mod state;

/// Application logs plugin support (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub mod application_logs;

// ============================================================================
// Runtime Components (requires `runtime` feature)
// ============================================================================

/// Actor runtime for async components (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub mod actors;

/// Re-export actors as "akka" for C# compatibility (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub use actors as akka;

/// System management and orchestration (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub mod neo_system;

/// Oracle service implementation (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub mod oracle_service;

/// Token tracking service (requires `runtime` feature).
#[cfg(feature = "runtime")]
pub mod tokens_tracker;

// ============================================================================
// Public Re-exports
// ============================================================================

// Core types
pub use big_decimal::BigDecimal;
pub use builders::{
    AndConditionBuilder, OrConditionBuilder, SignerBuilder, TransactionAttributesBuilder,
    TransactionBuilder, WitnessBuilder, WitnessConditionBuilder, WitnessRuleBuilder,
};
pub use contains_transaction_type::ContainsTransactionType;
pub use cryptography::{ECCurve, ECPoint};
pub use error::{CoreError, CoreResult, Result, UnifiedError, UnifiedResult};
pub use events::{EventHandler, EventManager};
pub use hardfork::Hardfork;
pub use ledger::{Block, BlockHeader};
pub use neo_primitives::{
    InvalidWitnessScopeError, UINT160_SIZE, UINT256_SIZE, UInt160, UInt256, WitnessScope,
};
pub use network::p2p::payloads::{
    HEADER_SIZE, InventoryType, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE,
    OracleResponseCode, Signer, Transaction, TransactionAttribute, TransactionAttributeType,
};
pub use protocol_settings::ProtocolSettings;
pub use rpc::RpcException;
pub use smart_contract::native::NativeContract;
pub use smart_contract::{Contract, ContractManifest, ContractParameterType, ContractState};
pub use time_provider::TimeProvider;
pub use unhandled_exception_policy::UnhandledExceptionPolicy;
pub use wallets::{KeyPair, Wallet};
pub use witness::Witness;
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};

// Merged module re-exports
pub use chain::{
    BlockIndex, BlockIndexEntry, BlockValidator, ChainError, ChainEvent, ChainEventSubscriber,
    ChainResult, ChainState, ChainStateSnapshot, ForkChoice, ValidationResult,
};
pub use config::{
    CONFIG_VERSION, ConfigError, ConfigResult, ConsensusSettings, GenesisConfig, GenesisValidator,
    LoggingSettings, NetworkConfig, NetworkType, NodeSettings, RpcSettings, Settings,
    StorageSettings, TelemetrySettings,
};
pub use mempool::{
    DEFAULT_EXPIRATION_BLOCKS, DEFAULT_MAX_TRANSACTIONS, FeePolicy, Mempool, MempoolConfig,
    MempoolError, MempoolResult, TransactionEntry, TransactionEntryParams,
};
pub use state::{
    AccountState, ContractStorage, MAX_SNAPSHOT_DEPTH, MemoryMptStore, MemoryWorldState,
    MutableStateView, SnapshotManager, SnapshotState, StateChanges, StateError, StateMut,
    StateResult, StateSnapshot, StateTrieManager, StateView, StorageChange, StorageItem,
    StorageKey, WorldState,
};

// Runtime types (requires `runtime` feature)
#[cfg(feature = "runtime")]
pub use neo_system::NeoSystem;

// ============================================================================
// Configuration Re-export
// ============================================================================

/// Protocol constants and configuration.
pub mod neo_config {
    pub use crate::constants::*;
}

// ============================================================================
// Network Types
// ============================================================================

pub use network::p2p::messages::{
    MessageHeader as NetworkMessageHeader, NetworkMessage, ProtocolMessage,
};
pub use network::{NetworkError, NetworkResult};

// ============================================================================
// I/O Re-export
// ============================================================================

/// I/O utilities with extension traits.
pub mod neo_io {
    pub use ::neo_io_crate::{
        BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
        serializable::{self, helper},
    };
    pub use Serializable as ISerializable;

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

// ============================================================================
// VM Re-export
// ============================================================================

/// Re-export of Neo Virtual Machine types used internally by neo-core modules.
pub mod neo_vm {
    pub use neo_vm::{CallFlags, OpCode, ScriptBuilder, StackItem, VMState};
}

// ============================================================================
// Ledger Re-export
// ============================================================================

/// Re-export of ledger types.
pub mod neo_ledger {
    pub use crate::ledger::{
        block::Block, block_header::BlockHeader,
        blockchain_application_executed::ApplicationExecuted, header_cache::HeaderCache,
        memory_pool::MemoryPool, verify_result::VerifyResult,
    };
}

// ============================================================================
// Foundation Crate Re-exports
// ============================================================================

/// Re-exports from [`neo_primitives`] crate.
///
/// Contains core primitive types like `UInt160`, `UInt256`.
/// Kept for backward compatibility; no current downstream consumers.
pub mod primitives {
    pub use neo_primitives::{UInt160, UInt256};
}

/// Re-exports from [`neo_crypto`] crate.
///
/// Contains cryptographic primitives and hash functions.
/// Kept for backward compatibility; no current downstream consumers.
pub mod crypto {
    pub use neo_crypto::{Crypto, CryptoError, ECC, HashAlgorithm, ct_hash_eq, ct_hash_slice_eq};
}

/// Re-exports from [`neo_storage`] crate.
///
/// Contains storage traits and abstractions.
/// Kept for backward compatibility; no current downstream consumers.
pub mod storage {
    pub use neo_storage::{
        IReadOnlyStore, ISnapshot, IStore, IWriteStore, StorageItem, StorageKey,
    };
}

/// Re-exports smart contract types for backward compatibility.
///
/// Note: `neo-contract` crate has been merged - types now live in [`smart_contract`] module.
/// No current downstream consumers use this re-export path.
pub mod contract {
    pub use crate::smart_contract::*;
}

// ============================================================================
// IVerifiable Trait
// ============================================================================

/// Trait for verifiable blockchain objects.
///
/// This trait defines the interface for objects that can be cryptographically
/// verified, such as blocks and transactions. It consolidates witness-handling
/// behaviour from C# `IVerifiable` with the helper methods required by the
/// runtime.
///
/// # Implementors
///
/// - [`Block`]
/// - [`Transaction`]
/// - [`Header`](ledger::BlockHeader)
///
/// # Example
///
/// ```rust,no_run
/// use neo_core::IVerifiable;
/// use neo_primitives::UInt256;
///
/// fn verify_and_hash<T: IVerifiable>(item: &T) -> Option<UInt256> {
///     if item.verify() {
///         item.hash().ok()
///     } else {
///         None
///     }
/// }
/// ```
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

    /// Gets the script hashes that should be verified for this container.
    fn get_script_hashes_for_verifying(
        &self,
        snapshot: &crate::persistence::DataCache,
    ) -> Vec<UInt160>;

    /// Gets the witnesses associated with this container.
    fn get_witnesses(&self) -> Vec<&Witness>;

    /// Gets mutable access to the witnesses associated with this container.
    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness>;

    /// Verifies the witnesses with the supplied gas limit.
    fn verify_witnesses(
        &self,
        settings: &ProtocolSettings,
        snapshot: &crate::persistence::DataCache,
        max_gas: i64,
    ) -> bool
    where
        Self: Sized,
    {
        crate::smart_contract::helper::Helper::verify_witnesses(self, settings, snapshot, max_gas)
    }

    /// Attempts to view this verifiable container as a transaction.
    fn as_transaction(&self) -> Option<&crate::network::p2p::payloads::Transaction> {
        self.as_any().downcast_ref()
    }

    /// Returns a reference to self as `Any` for downcasting.
    ///
    /// This enables runtime type checking and downcasting to concrete types.
    ///
    /// # Returns
    ///
    /// A reference to self as a trait object.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
