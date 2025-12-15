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

// Declarative macros for reducing boilerplate (must be declared before other modules)
#[macro_use]
pub mod macros;

// Module declarations with documentation
// Advanced metrics moved to neo-monitoring crate
/// Big decimal arithmetic implementation
pub mod big_decimal;
// Block moved to ledger module
/// Builder pattern implementations for complex types
pub mod builders;
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
/// Commonly used type re-exports
pub mod prelude;
/// Protocol settings configuration (matches C# ProtocolSettings)
pub mod protocol_settings;
// System metrics moved to neo-monitoring crate
// Monitoring moved to neo-monitoring crate
// Neo system management moved to neo-node crate (Phase 2 refactoring)
// The neo_system module contains runtime orchestration code that depends on actors.
// It will be available in neo-node which provides the full runtime.
// pub mod neo_system;
// Transaction signer moved to sign module
// Transaction structures moved to ledger module
// Transaction type definitions moved to ledger module
// Transaction validation moved to ledger module
/// Witness verification system
pub mod witness;
/// Witness rule evaluation
pub mod witness_rule;

// Monitoring (feature-gated)
#[cfg(feature = "monitoring")]
pub mod monitoring;

// Telemetry module for metrics and observability
pub mod telemetry;

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
// Events module (matches C# Neo.Events)
pub mod events;
// IO module (matches C# Neo.IO)
pub mod io;
/// Shared RPC models and helpers.
pub mod rpc;
// Time provider module (matches C# Neo.TimeProvider)
pub mod time_provider;
// State service module (matches C# Neo.Plugins.StateService)
pub mod state_service;
// Typed service interfaces for shared subsystems
pub mod services;

// Actor runtime moved to neo-node crate (Phase 2 refactoring)
// The actors module and ractor dependency have been removed from neo-core
// to keep this crate as a pure protocol layer without async runtime dependencies.
//
// If you need actor functionality, use neo-node which provides the runtime.
// pub mod actors;
// pub use actors as akka;

// Tokens tracker module moved to neo-node (Phase 2 refactoring)
// It depends on NeoSystem runtime which is now in neo-node
// pub mod tokens_tracker;

// Re-exports for convenient access
pub use big_decimal::BigDecimal;
pub use builders::{SignerBuilder, TransactionBuilder, WitnessBuilder};
pub use contains_transaction_type::ContainsTransactionType;
pub use cryptography::{ECCurve, ECPoint};
pub use error::{CoreError, CoreResult, Result};
pub use events::{EventHandler, EventManager};
pub use hardfork::Hardfork;
pub use ledger::{Block, BlockHeader};
pub use neo_primitives::{
    InvalidWitnessScopeError, UInt160, UInt256, WitnessScope, UINT160_SIZE, UINT256_SIZE,
};
// NeoSystem moved to neo-node crate
// pub use neo_system::NeoSystem;
pub use network::p2p::payloads::{
    InventoryType, OracleResponseCode, Signer, Transaction, TransactionAttribute,
    TransactionAttributeType, HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE,
};
pub use protocol_settings::ProtocolSettings;
pub use rpc::RpcException;
pub use smart_contract::native::NativeContract;
pub use smart_contract::{Contract, ContractManifest, ContractParameterType, ContractState};
pub use time_provider::TimeProvider;
pub use wallets::{KeyPair, Wallet};
pub use witness::Witness;
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};

// Compatibility re-export moved to neo-node crate
// pub mod system {
//     pub use crate::neo_system::*;
// }

// NOTE: Global blockchain and store singletons will be implemented
// when the proper types are available in their respective modules.
// The C# implementation has:
// - Blockchain.Singleton
// - Store.Singleton
// These will be properly typed once all dependencies are in place.

/// Trait for verifiable blockchain objects.
///
/// This trait defines the interface for objects that can be cryptographically
/// verified, such as blocks and transactions. It consolidates witness-handling
/// behaviour from C# `IVerifiable` with the helper methods required by the
/// runtime (hashing, serialization helpers, type erasure).
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

pub mod neo_config {
    pub use crate::constants::*;
}

pub use network::p2p::messages::{
    MessageHeader as NetworkMessageHeader, NetworkMessage, ProtocolMessage,
};
pub use network::{NetworkError, NetworkResult};

pub mod neo_io {
    pub use ::neo_io_crate::{
        serializable::{self, helper},
        BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
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

pub mod neo_crypto {
    use crate::cryptography::NeoHash;

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
    use crate::cryptography::NeoHash;
    use crate::UInt256;

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
        use crate::cryptography::NeoHash;

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
        use super::{ECCurve, ECPoint};

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
            ECPoint::from_bytes_with_curve(curve, data).map_err(|e| ECCError(e.to_string()))
        }

        pub fn decode_compressed_point(data: &[u8]) -> Result<ECPoint, ECCError> {
            ECPoint::from_bytes(data).map_err(|e| ECCError(e.to_string()))
        }
    }

    pub use crate::cryptography::{ECCurve, ECPoint};

    #[derive(Clone)]
    struct MerkleTreeNode {
        hash: UInt256,
        left: Option<Box<MerkleTreeNode>>,
        right: Option<Box<MerkleTreeNode>>,
    }

    impl MerkleTreeNode {
        fn leaf(hash: UInt256) -> Self {
            Self {
                hash,
                left: None,
                right: None,
            }
        }

        fn is_pruned(&self) -> bool {
            self.left.is_none() && self.right.is_none()
        }
    }

    /// Merkle tree implementation used across the network layer.
    pub struct MerkleTree {
        root: Option<Box<MerkleTreeNode>>,
        depth: usize,
    }

    impl MerkleTree {
        /// Builds a merkle tree from the supplied hashes.
        pub fn new(hashes: &[UInt256]) -> Self {
            if hashes.is_empty() {
                return Self {
                    root: None,
                    depth: 0,
                };
            }

            let mut nodes: Vec<MerkleTreeNode> =
                hashes.iter().copied().map(MerkleTreeNode::leaf).collect();

            let mut depth = 1;
            while nodes.len() > 1 {
                let mut parents = Vec::with_capacity(nodes.len().div_ceil(2));
                let mut index = 0;
                while index < nodes.len() {
                    let left = nodes[index].clone();
                    let right = if index + 1 < nodes.len() {
                        nodes[index + 1].clone()
                    } else {
                        left.clone()
                    };

                    let hash = hash_pair(&left.hash, &right.hash);
                    parents.push(MerkleTreeNode {
                        hash,
                        left: Some(Box::new(left)),
                        right: Some(Box::new(right)),
                    });

                    index += 2;
                }
                nodes = parents;
                depth += 1;
            }

            Self {
                root: nodes.pop().map(Box::new),
                depth,
            }
        }

        /// Returns the depth of the tree (leaf-only trees report depth 1).
        pub fn depth(&self) -> usize {
            self.depth
        }

        /// Computes the merkle root for the supplied hashes.
        ///
        /// Performance: Uses an optimized in-place algorithm that avoids building
        /// the full tree structure. Only allocates a single working buffer.
        /// Time complexity: O(n), Space complexity: O(n) where n = number of hashes.
        pub fn compute_root(hashes: &[UInt256]) -> Option<UInt256> {
            if hashes.is_empty() {
                return None;
            }
            if hashes.len() == 1 {
                return Some(hashes[0]);
            }

            // Work buffer - we'll reduce this in-place level by level
            let mut current: Vec<UInt256> = hashes.to_vec();

            while current.len() > 1 {
                let mut next = Vec::with_capacity(current.len().div_ceil(2));
                let mut i = 0;
                while i < current.len() {
                    let left = &current[i];
                    // If odd number of elements, duplicate the last one
                    let right = current.get(i + 1).unwrap_or(left);
                    next.push(hash_pair(left, right));
                    i += 2;
                }
                current = next;
            }

            current.pop()
        }

        /// Computes the merkle root by building the full tree.
        /// Use this when you need the tree structure for trimming or proof generation.
        pub fn compute_root_with_tree(hashes: &[UInt256]) -> Option<UInt256> {
            let tree = Self::new(hashes);
            tree.root().copied()
        }

        /// Returns the root hash when available.
        pub fn root(&self) -> Option<&UInt256> {
            self.root.as_ref().map(|node| &node.hash)
        }

        /// Trims the tree according to the provided bloom-filter flags.
        ///
        /// Flags represent which leaves should be retained. When both leaves under
        /// a node are excluded the branch is pruned and replaced by the parent hash.
        pub fn trim(&mut self, flags: &[bool]) {
            let Some(root) = self.root.as_mut() else {
                return;
            };

            if self.depth <= 1 {
                return;
            }

            let required = 1usize << (self.depth - 1);
            let mut padded = vec![false; required];
            for (index, flag) in flags.iter().enumerate().take(required) {
                padded[index] = *flag;
            }

            trim_node(root, 0, self.depth, &padded);
        }

        /// Returns the hashes in depth-first order.
        pub fn to_hash_array(&self) -> Vec<UInt256> {
            let mut hashes = Vec::new();
            if let Some(root) = self.root.as_ref() {
                depth_first_collect(root, &mut hashes);
            }
            hashes
        }
    }

    fn depth_first_collect(node: &MerkleTreeNode, hashes: &mut Vec<UInt256>) {
        if node.left.is_none() {
            hashes.push(node.hash);
        } else {
            if let Some(left) = node.left.as_ref() {
                depth_first_collect(left, hashes);
            }
            if let Some(right) = node.right.as_ref() {
                depth_first_collect(right, hashes);
            }
        }
    }

    fn trim_node(node: &mut MerkleTreeNode, index: usize, depth: usize, flags: &[bool]) {
        if depth <= 1 || node.left.is_none() {
            return;
        }

        if depth == 2 {
            let left_flag = flags.get(index * 2).copied().unwrap_or(false);
            let right_flag = flags.get(index * 2 + 1).copied().unwrap_or(false);

            if !left_flag && !right_flag {
                node.left = None;
                node.right = None;
            }
            return;
        }

        if let Some(left) = node.left.as_mut() {
            trim_node(left, index * 2, depth - 1, flags);
        }
        if let Some(right) = node.right.as_mut() {
            trim_node(right, index * 2 + 1, depth - 1, flags);
        }

        let left_pruned = node
            .left
            .as_ref()
            .map(|child| child.is_pruned())
            .unwrap_or(true);
        let right_pruned = node
            .right
            .as_ref()
            .map(|child| child.is_pruned())
            .unwrap_or(true);

        if left_pruned && right_pruned {
            node.left = None;
            node.right = None;
        }
    }

    fn hash_pair(left: &UInt256, right: &UInt256) -> UInt256 {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&left.to_array());
        bytes[32..].copy_from_slice(&right.to_array());
        UInt256::from(NeoHash::hash256(&bytes))
    }
}

pub mod neo_vm {
    pub use neo_vm::*;
}

pub mod neo_ledger {
    pub use crate::ledger::{
        block::Block, block_header::BlockHeader,
        // Blockchain moved to neo-node (Phase 2 refactoring)
        // blockchain::Blockchain,
        blockchain_application_executed::ApplicationExecuted, header_cache::HeaderCache,
        memory_pool::MemoryPool, verify_result::VerifyResult,
    };
}

// === Re-exports from new foundation crates ===
// These provide backward compatibility while types are migrated to their new homes.

/// Re-exports from neo-primitives crate.
/// Contains core primitive types like UInt160, UInt256.
pub mod primitives {
    pub use neo_primitives::*;
}

/// Re-exports from neo-crypto crate.
/// Contains cryptographic primitives and hash functions.
pub mod crypto {
    pub use neo_crypto::*;
}

/// Re-exports from neo-storage crate.
/// Contains storage traits and abstractions.
pub mod storage {
    pub use neo_storage::*;
}

/// Re-exports smart contract types for backward compatibility.
/// Contains smart contract types and execution engine components.
/// Note: neo-contract crate has been merged - types now live in smart_contract module.
pub mod contract {
    pub use crate::smart_contract::*;
}

// NOTE: neo-p2p and neo-consensus are NOT re-exported here.
// This is intentional to maintain proper layering:
// - neo-core is Layer 1 (Core)
// - neo-p2p and neo-consensus are Layer 2 (Protocol)
// Import directly from neo-p2p and neo-consensus crates instead.
