//! # neo-payloads::transaction
//!
//! Transaction body, signer, witness, and fee records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `json`: JSON models and codecs for external service integration.
//! - `traits`: transaction trait implementations.
//! - `serialization`: serialization codecs and compatibility checks.

use crate::{
    InventoryType, TransactionAttributeType, inventory::Inventory, signer::Signer,
    transaction_attribute::TransactionAttribute, witness::Witness,
};
use base64::{Engine as _, engine::general_purpose};
use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_error::CoreResult;
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use neo_vm::Interoperable;
use parking_lot::Mutex;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::{Hash as StdHash, Hasher};

/// The maximum size of a transaction.
pub const MAX_TRANSACTION_SIZE: usize = 102400;

/// The maximum number of attributes that can be contained within a transaction.
pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;

/// The size of a transaction header.
pub const HEADER_SIZE: usize = 1 + 4 + 8 + 8 + 4; // Version + Nonce + SystemFee + NetworkFee + ValidUntilBlock

/// Represents a transaction.
#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    /// Version of the transaction format.
    pub(super) version: u8,

    /// Random number to avoid hash collision.
    pub(super) nonce: u32,

    /// System fee in datoshi (1 datoshi = 1e-8 GAS).
    pub(super) system_fee: i64,

    /// Network fee in datoshi (1 datoshi = 1e-8 GAS).
    pub(super) network_fee: i64,

    /// Block height when transaction expires.
    pub(super) valid_until_block: u32,

    /// Signers of the transaction.
    pub(super) signers: Vec<Signer>,

    /// Attributes of the transaction.
    pub(super) attributes: Vec<TransactionAttribute>,

    /// Script to be executed.
    pub(super) script: Vec<u8>,

    /// Witnesses for verification.
    pub(super) witnesses: Vec<Witness>,

    #[serde(skip)]
    pub(super) _hash: Mutex<Option<UInt256>>,

    #[serde(skip)]
    pub(super) _size: Mutex<Option<usize>>,
}

impl Clone for Transaction {
    fn clone(&self) -> Self {
        let cached_hash = *self._hash.lock();
        let cached_size = *self._size.lock();

        Self {
            version: self.version,
            nonce: self.nonce,
            system_fee: self.system_fee,
            network_fee: self.network_fee,
            valid_until_block: self.valid_until_block,
            signers: self.signers.clone(),
            attributes: self.attributes.clone(),
            script: self.script.clone(),
            witnesses: self.witnesses.clone(),
            _hash: Mutex::new(cached_hash),
            _size: Mutex::new(cached_size),
        }
    }
}

// Include implementation files
mod core;
mod json;
mod traits;

// ============================================================================
// ============================================================================

// The transaction wire size is provided by the canonical `Serializable::size`
// impl (see `serialization.rs`), which includes the version (1 byte), the
// script var-int length prefix, and the witnesses var-array. A previous
// inherent `Transaction::size` shadowed it with an undersized value (version
// as 4 bytes, no script prefix, witnesses omitted), corrupting `fee_per_byte`
// (mempool ordering) and the RPC `size` field — callers now use the trait.

impl neo_primitives::Verifiable for Transaction {
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<neo_primitives::UInt256> {
        self.try_hash().map_err(|e| {
            neo_primitives::error::PrimitiveError::invalid_data(format!(
                "transaction hash failed: {e}"
            ))
        })
    }

    fn hash_data(&self) -> Vec<u8> {
        let mut writer = neo_io::BinaryWriter::new();
        if self.serialize_unsigned(&mut writer).is_err() {
            return Vec::new();
        }
        writer.into_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Lightweight pre-verification that the transaction is structurally valid.
    ///
    /// Full signature and state-dependent verification is deferred to
    /// `TransactionRouter::preverify()` → `verify_state_independent()`. This
    /// method performs basic structural checks: the version must be valid,
    /// signers must not be empty, and at least one witness must be present.
    /// Returns `false` for transactions that are structurally invalid at the
    /// protocol level.
    fn verify(&self) -> bool {
        // Reject transactions with an unknown version (C# only supports version 0).
        if self.version > 0 {
            return false;
        }
        // Every transaction must have at least one signer and at least one witness.
        if self.signers.is_empty() || self.witnesses.is_empty() {
            return false;
        }
        true
    }
}

impl crate::VerifiableExt for Transaction {
    fn script_hashes_for_verifying(
        &self,
        _snapshot: &neo_storage::DataCache,
    ) -> Vec<neo_primitives::UInt160> {
        self.signers().iter().map(|s| s.account).collect()
    }
    fn witnesses(&self) -> Vec<&crate::Witness> {
        self.witnesses.iter().collect()
    }
    fn witnesses_mut(&mut self) -> Vec<&mut crate::Witness> {
        self.witnesses.iter_mut().collect()
    }
    /// A transaction is its own verification script container. Returning `Some`
    /// here makes witness verification install the real `Transaction` (with its
    /// signers, witness scopes/rules, and OracleResponse attributes) as the
    /// engine's script container — matching C# `Helper.VerifyWitness`, which
    /// passes the `IVerifiable` itself. Without this override the engine would
    /// fall back to a hash-only wrapper and `CheckWitness` could not see the
    /// signers during verification, wrongly rejecting contract-account,
    /// witness-rule, and OracleResponse transactions that C# accepts.
    fn as_transaction(&self) -> Option<&crate::Transaction> {
        Some(self)
    }

    fn to_verifiable_container(&self) -> Option<std::sync::Arc<dyn neo_primitives::Verifiable>> {
        Some(std::sync::Arc::new(self.clone()))
    }
}
mod serialization;
