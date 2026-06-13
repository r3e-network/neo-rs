// Copyright (C) 2015-2025 The Neo Project.
//
// transaction/mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    InventoryType, TransactionAttributeType, inventory::Inventory, signer::Signer,
    transaction_attribute::TransactionAttribute, witness::Witness,
};
use base64::{Engine as _, engine::general_purpose};
use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_error::CoreResult;
use neo_io::serializable::helper::{
    deserialize_array_with, deserialize_exact_array, get_var_size, get_var_size_bytes,
    get_var_size_serializable_slice, serialize_array,
};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use neo_vm::Interoperable;
use neo_vm::StackItem;
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
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
        }
    }
}

// Include implementation files
mod core;
mod json;
mod traits;

// ============================================================================
// ============================================================================

impl Transaction {
    fn size(&self) -> usize {
        let attr_bytes_size: usize = self.attributes.iter().map(|a| a.size()).sum();
        let signers_size: usize = self.signers.iter().map(|s| s.size()).sum();
        4 + 4
            + 8
            + 8
            + 4
            + 4
            + self.script.len()
            + get_var_size(attr_bytes_size as u64)
            + attr_bytes_size
            + get_var_size(signers_size as u64)
            + signers_size
    }
}

impl neo_primitives::Verifiable for Transaction {
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<neo_primitives::UInt256> {
        let data = self.try_get_hash_data().map_err(|e| {
            neo_primitives::error::PrimitiveError::invalid_data(format!(
                "transaction unsigned serialization failed: {e}"
            ))
        })?;
        Ok(neo_primitives::UInt256::from(neo_crypto::Crypto::sha256(
            &data,
        )))
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

    fn verify(&self) -> bool {
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
        Vec::new()
    }
    fn witnesses_mut(&mut self) -> Vec<&mut crate::Witness> {
        Vec::new()
    }
}
mod serialization;
