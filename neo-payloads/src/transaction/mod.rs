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
//! - `serialization`: serialization codecs and compatibility checks.
//! - `traits`: stack, inventory, payload, default, and hash trait adapters.
//! - `verification`: witness-verification and hash-container adapters.

use crate::{
    InventoryType, TransactionAttributeType, inventory::Inventory, signer::Signer,
    transaction_attribute::TransactionAttribute, witness::Witness,
};
use base64::{Engine as _, engine::general_purpose};
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
mod serialization;
mod traits;
mod verification;
