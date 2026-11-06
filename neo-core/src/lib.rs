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
//! This crate is **Layer 2 (VM and protocol)**. Dependencies stay in this
//! layer or go downward. Services (`neo-rpc`) and the node are above it.
//!
//! ```text
//! Layer 2 (VM and protocol): neo-vm, neo-core, neo-p2p, neo-consensus
//!            │
//!            ▼
//! Layer 1 (Crypto): neo-crypto
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives, neo-io, neo-storage, neo-json, neo-config
//! ```
//!
//! ## Dependencies
//!
//! - [`neo_primitives`]: Core types (`UInt160`, `UInt256`)
//! - [`neo_crypto`]: Cryptographic operations
//! - [`neo_storage`]: Storage traits
//! - [`neo_io`]: I/O operations
//! - [`neo_json`]: JSON handling
//! - [`neo_config`]: Protocol configuration, including hardfork heights
//! - [`neo_vm`]: Execution engine
//! - `neo-p2p`: P2P message vocabulary shared with the node
//!
//! ## Features
//!
//! - `runtime`: re-exports the [`neo_actors`] crate as [`actors`] and [`runtime`]
//!   for the actor-based node components living in `neo-system`
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
//! ## Verifiable Trait
//!
//! The [`Verifiable`] trait is central to blockchain validation. It is implemented
//! by types that can be cryptographically verified, such as blocks and transactions:
//!
//! ```rust,no_run
//! use neo_core::Verifiable;
//!
//! fn verify_container<T: Verifiable>(container: &T) -> bool {
//!     container.verify()
//! }
//! ```

// Public API documentation is covered incrementally; keep rustdoc warnings
// visible without allowing this crate to hide newly introduced lint failures.
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Self-reference for macro exports
pub extern crate neo_config as neo_config_crate;
extern crate self as neo_core;

// ============================================================================
// Declarative Macros & Validation Extension Traits (re-exported)
// ============================================================================

// The declarative boilerplate macros moved to `neo-primitives` and the
// `OptionExt`/`ValidateLength` extension traits moved to `neo-io` (they are
// defined in terms of `IoError`/`IoResult`, which that crate owns).
// Re-exported here for backward compatibility with the historical
// `neo_core::` paths (e.g. `crate::impl_error_from!`,
// `neo_core::impl_ord_by_fields!`, `crate::macros::ValidateLength`).
pub use ::neo_io_crate::extensions::validation::{OptionExt, ValidateLength};
pub use ::neo_primitives::{
    impl_default_via_new, impl_error_from, impl_from_bytes, impl_hash_for_fields,
    impl_ord_by_fields,
};

// ============================================================================
// Foundation Modules
// ============================================================================

/// Builder pattern implementations for complex types.
///
/// Contains builders for `Transaction`, `Signer`, `Witness`, and witness conditions.
/// The authoritative implementation lives in `neo-protocol`; re-exported here for
/// backward compatibility with the historical `neo_core::builders` path.
pub use neo_protocol::builders;

/// Script bytecode validation helpers backed by external NeoVM metadata.
pub mod script_validation {
    pub use ::neo_vm::{
        ScriptInstruction, ValidatedScript, ValidationResult, parse_script_instructions,
        validate_script, validate_strict_script,
    };
}

/// System-wide protocol constants.
///
/// Network magic numbers, port defaults, fee constants, and size limits.
pub mod constants;

/// Compatibility module for the historical transaction containment path.
pub mod contains_transaction_type {
    pub use neo_primitives::contains_transaction_type::*;
}

/// Transaction type containment checking.
///
/// Provides utilities for checking if a transaction contains specific types.
/// Core error types and error handling utilities.
/// Core error types and error handling utilities.
///
/// This module provides comprehensive error handling for Neo core operations,
/// including serialization errors, validation failures, and system errors.
pub mod error;

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
pub use neo_protocol::hardfork;

/// Commonly used type re-exports.
pub mod prelude;

/// Protocol settings and network configuration.
///
/// Matches C# `ProtocolSettings` class.
pub use neo_protocol::protocol_settings;

/// Witness verification system.
///
/// Handles script verification for transactions.
pub mod witness;

/// Witness rule evaluation for conditional verification.
pub mod witness_rule;

// ============================================================================
// Optional Features
// ============================================================================

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

/// Extension methods and utilities.
///
/// Matches C# `Neo.Extensions` namespace.
pub mod extensions;

// The `events` module (event manager + plugin broadcast) and the consolidated
// `i_event_handlers` trait aliases moved to `neo-system` (task W3-C2) together
// with the `NeoSystem` runtime; consumers now use `neo_system::events` and
// `neo_system::i_event_handlers`. This crate cannot re-export them because
// `neo-system` depends on `neo-core`.

/// RPC exception type (re-exported from `neo_primitives`).
///
/// The former `rpc/` module was a 5-line shim; the type is now re-exported
/// directly at the crate root and under a `rpc` namespace alias for path compat.
pub mod rpc {
    pub use neo_primitives::RpcException;
}

/// Time provider abstraction for testability.
///
/// Re-exported from [`neo_primitives`], the single source of truth for the
/// pluggable time source (`TimeProvider` / `TimeSource`).
pub mod time_provider {
    pub use neo_primitives::time_provider::*;
}

/// State service for world state management.
pub mod state_service;

/// Service trait definitions for dependency injection.
pub mod services;

// ============================================================================
// Runtime Components (requires `runtime` feature)
// ============================================================================

/// Actor runtime for async components (requires `runtime` feature).
///
/// Task 194: the local `actors/` module was removed. This is now a re-export of
/// the protocol-agnostic [`neo_actors`] crate, which exposes a public
/// `ActorRef::new` construction seam (used by `neo-system`'s P2P actor tests).
#[cfg(feature = "runtime")]
pub use neo_actors as actors;

/// Neutral actor runtime facade for async node components (requires `runtime` feature).
///
/// Task W3-C2: the system orchestration (`neo_system`), the blockchain actor
/// implementation, the P2P actors, the oracle service and the event system moved
/// to `neo-system`; this facade stays because `neo-system` reaches the actor
/// runtime through `neo_core::runtime` so both crates share one `TypeId`.
#[cfg(feature = "runtime")]
pub use neo_actors as runtime;

// ============================================================================
// Public Re-exports
// ============================================================================

// Core types
pub use builders::{
    AndConditionBuilder, OrConditionBuilder, SignerBuilder, TransactionAttributesBuilder,
    TransactionBuilder, WitnessBuilder, WitnessConditionBuilder, WitnessRuleBuilder,
};
pub use cryptography::{ECCurve, ECPoint};
pub use error::{CoreError, CoreResult, Result};
pub use hardfork::Hardfork;
pub use ledger::{Block, BlockHeader};
pub use neo_primitives::RpcException;
pub use neo_primitives::{
    BigDecimal, ContainsTransactionType, UnhandledExceptionPolicy, panic_message,
};
pub use neo_primitives::{
    InvalidWitnessScopeError, UINT160_SIZE, UINT256_SIZE, UInt160, UInt256, WitnessScope,
};
pub use neo_vm::{ScriptBuilder, rpc_json};
pub use network::p2p::payloads::{
    BlockExt, HEADER_SIZE, HeaderExt, InventoryType, MAX_TRANSACTION_ATTRIBUTES,
    MAX_TRANSACTION_SIZE, OracleResponseCode, Signer, Transaction, TransactionAttribute,
    TransactionAttributeType, TransactionExt,
};
pub use protocol_settings::ProtocolSettings;
pub use smart_contract::native::NativeContract;
pub use smart_contract::{Contract, ContractManifest, ContractParameterType, ContractState};
pub use time_provider::TimeProvider;

pub use wallets::{KeyPair, Wallet};
pub use witness::{Witness, WitnessData, WitnessExt};
pub use witness_rule::{
    ToStackItem, WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction,
};

// ============================================================================
// Configuration Re-export
// ============================================================================

/// Protocol constants and configuration.
pub mod neo_config {
    pub use crate::constants::*;
}

/// Backward-compatible re-export of the `neo-config` crate.
pub use neo_config_crate as config;

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
    pub use crate::extensions::io::SerializableExtensions as SerializableExt;
    pub use ::neo_io_crate::{
        BinaryWriter, IoError, IoResult, MemoryReader, Serializable, impl_serializable,
        serializable::{self, helper},
    };
}

// ============================================================================
// VM Compatibility Surface
// ============================================================================

/// Neo VM compatibility types used by core and downstream crates.
pub use ::neo_vm;
pub use ::neo_vm::script_builder;

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

/// Re-exports from [`neo_crypto`] crate.
pub use neo_crypto::{Crypto, CryptoError, ECC, HashAlgorithm, ct_hash_eq, ct_hash_slice_eq};

/// Re-exports from [`neo_storage`] crate.
pub use neo_storage::{StorageItem, StorageKey};

// ============================================================================
// Verifiable Trait
// ============================================================================

// Re-export the base Verifiable trait from neo-primitives.
pub use neo_primitives::Verifiable;

/// Extension of [`neo_primitives::Verifiable`] with neo-core-specific methods
/// that depend on `DataCache`, `ProtocolSettings`, and the smart contract engine.
///
/// # Implementors
///
/// - [`Block`]
/// - [`Transaction`]
/// - [`Header`](ledger::BlockHeader)
pub trait VerifiableExt: WitnessData {
    /// Gets the script hashes that should be verified for this container.
    fn script_hashes_for_verifying(&self, snapshot: &crate::persistence::DataCache)
    -> Vec<UInt160>;

    /// Attempts to view this verifiable container as a transaction.
    fn as_transaction(&self) -> Option<&crate::network::p2p::payloads::Transaction> {
        self.as_any().downcast_ref()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
