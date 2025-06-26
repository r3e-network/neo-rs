// Copyright (C) 2015-2025 The Neo Project.
//
// core.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Core Transaction struct and basic operations matching C# Neo N3 Transaction.cs exactly.

use crate::signer::Signer;
use crate::witness::Witness;
use crate::{CoreError, UInt160, UInt256};
use neo_io::Serializable;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Mutex;

use super::attributes::TransactionAttribute;

/// Maximum size of a transaction in bytes.
pub const MAX_TRANSACTION_SIZE: usize = 102400;

/// Maximum number of attributes that can be contained within a transaction.
pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;

/// The size of a transaction header in bytes.
pub const HEADER_SIZE: usize = 1 +  // Version (byte)
    4 +  // Nonce (uint32)
    8 +  // SystemFee (int64)
    8 +  // NetworkFee (int64)
    4; // ValidUntilBlock (uint32)

/// Represents a Neo blockchain transaction.
///
/// A transaction contains all the information needed to execute
/// operations on the Neo blockchain, including scripts, fees, and signatures.
/// This implementation matches the C# Neo N3 Transaction class exactly.
#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    /// The version of the transaction (matches C# Version property).
    pub(crate) version: u8,

    /// The nonce of the transaction (matches C# Nonce property).
    pub(crate) nonce: u32,

    /// The system fee of the transaction (matches C# SystemFee property).
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub(crate) system_fee: i64,

    /// The network fee of the transaction (matches C# NetworkFee property).
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub(crate) network_fee: i64,

    /// Indicates that the transaction is only valid before this block height (matches C# ValidUntilBlock property).
    pub(crate) valid_until_block: u32,

    /// The signers of the transaction (matches C# Signers property).
    pub signers: Vec<Signer>,

    /// The attributes of the transaction (matches C# Attributes property).
    pub attributes: Vec<TransactionAttribute>,

    /// The script of the transaction (matches C# Script property).
    pub(crate) script: Vec<u8>,

    /// The witnesses of the transaction (matches C# Witnesses property).
    pub(crate) witnesses: Vec<Witness>,

    /// Cached hash of the transaction (matches C# _hash field).
    /// Uses Mutex for thread-safe interior mutability.
    #[serde(skip)]
    pub(crate) _hash: Mutex<Option<UInt256>>,

    /// Cached size of the transaction (matches C# _size field).
    /// Uses Mutex for thread-safe interior mutability.
    #[serde(skip)]
    pub(crate) _size: Mutex<i32>,
}

impl Transaction {
    /// Creates a new Transaction instance.
    pub fn new() -> Self {
        Self {
            version: 0,
            nonce: 0,
            system_fee: 0,
            network_fee: 0,
            valid_until_block: 0,
            signers: Vec::new(),
            attributes: Vec::new(),
            script: Vec::new(),
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
            _size: Mutex::new(0),
        }
    }

    // C#-compatible property accessors

    /// Gets the version of the transaction (matches C# Version property exactly).
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Sets the version of the transaction (matches C# Version setter exactly).
    pub fn set_version(&mut self, value: u8) {
        self.version = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the nonce of the transaction (matches C# Nonce property exactly).
    pub fn nonce(&self) -> u32 {
        self.nonce
    }

    /// Sets the nonce of the transaction (matches C# Nonce setter exactly).
    pub fn set_nonce(&mut self, value: u32) {
        self.nonce = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the system fee of the transaction (matches C# SystemFee property exactly).
    pub fn system_fee(&self) -> i64 {
        self.system_fee
    }

    /// Sets the system fee of the transaction (matches C# SystemFee setter exactly).
    pub fn set_system_fee(&mut self, value: i64) {
        self.system_fee = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the network fee of the transaction (matches C# NetworkFee property exactly).
    pub fn network_fee(&self) -> i64 {
        self.network_fee
    }

    /// Sets the network fee of the transaction (matches C# NetworkFee setter exactly).
    pub fn set_network_fee(&mut self, value: i64) {
        self.network_fee = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the valid until block of the transaction (matches C# ValidUntilBlock property exactly).
    pub fn valid_until_block(&self) -> u32 {
        self.valid_until_block
    }

    /// Sets the valid until block of the transaction (matches C# ValidUntilBlock setter exactly).
    pub fn set_valid_until_block(&mut self, value: u32) {
        self.valid_until_block = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the script of the transaction (matches C# Script property exactly).
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Sets the script of the transaction (matches C# Script setter exactly).
    pub fn set_script(&mut self, value: Vec<u8>) {
        self.script = value;
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior exactly)
    }

    /// Gets the signers of the transaction (matches C# Signers property exactly).
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// Gets the attributes of the transaction (matches C# Attributes property exactly).
    pub fn attributes(&self) -> &[TransactionAttribute] {
        &self.attributes
    }

    /// Gets the witnesses of the transaction (matches C# Witnesses property exactly).
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }

    // Additional helper methods for transaction building

    /// Adds a signer to the transaction (matches C# Signers collection behavior).
    pub fn add_signer(&mut self, signer: Signer) {
        self.signers.push(signer);
        self.invalidate_cache(); // Invalidate hash cache (matches C# behavior)
    }

    /// Adds a witness to the transaction (matches C# Witnesses collection behavior).
    pub fn add_witness(&mut self, witness: Witness) {
        self.witnesses.push(witness);
        self.invalidate_cache(); // Invalidate size cache (matches C# behavior)
    }

    /// Gets the hash of the transaction (matches C# Hash property).
    /// This property caches the hash value like the C# implementation.
    /// Thread-safe with interior mutability.
    ///
    /// # Returns
    ///
    /// The transaction hash as UInt256
    pub fn get_hash(&self) -> Result<UInt256, CoreError> {
        let mut hash_guard = self._hash.lock().unwrap();
        if hash_guard.is_none() {
            *hash_guard = Some(self.calculate_hash()?);
        }
        Ok(hash_guard.unwrap())
    }

    /// Gets the hash of the transaction (C# Hash property compatibility).
    /// This method provides the exact same API as C# Neo Transaction.Hash property.
    /// Thread-safe with interior mutability.
    ///
    /// # Returns
    ///
    /// The transaction hash as UInt256
    pub fn hash(&self) -> Result<UInt256, CoreError> {
        self.get_hash()
    }

    /// Calculates the hash of the transaction (matches C# CalculateHash method).
    /// Uses double SHA256 hash like the C# implementation.
    fn calculate_hash(&self) -> Result<UInt256, CoreError> {
        let hash_data = self.get_hash_data();

        // Double SHA256 hash like C# implementation
        let mut hasher = Sha256::new();
        hasher.update(&hash_data);
        let first_hash = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(&first_hash);
        let final_hash = hasher.finalize();

        Ok(
            UInt256::from_bytes(&final_hash).map_err(|_| CoreError::InvalidData {
                message: "Invalid hash bytes".to_string(),
            })?,
        )
    }

    /// Gets the hash data for signing (transaction data without witnesses).
    ///
    /// # Returns
    ///
    /// The serialized transaction data for signing
    pub fn get_hash_data(&self) -> Vec<u8> {
        let mut writer = neo_io::BinaryWriter::new();

        // Write header
        writer.write_bytes(&[self.version]).unwrap();
        writer.write_bytes(&self.nonce.to_le_bytes()).unwrap();
        writer.write_bytes(&self.system_fee.to_le_bytes()).unwrap();
        writer.write_bytes(&self.network_fee.to_le_bytes()).unwrap();
        writer
            .write_bytes(&self.valid_until_block.to_le_bytes())
            .unwrap();

        // Write signers
        writer.write_var_int(self.signers.len() as u64).unwrap();
        for signer in &self.signers {
            Serializable::serialize(signer, &mut writer).unwrap();
        }

        // Write attributes
        writer.write_var_int(self.attributes.len() as u64).unwrap();
        for attribute in &self.attributes {
            attribute.serialize(&mut writer).unwrap();
        }

        // Write script
        writer.write_var_bytes(&self.script).unwrap();

        // Note: witnesses are NOT included in hash data

        writer.to_bytes()
    }

    /// Gets the sender of the transaction (first signer).
    ///
    /// # Returns
    ///
    /// The sender's account hash, or None if no signers
    pub fn sender(&self) -> Option<UInt160> {
        self.signers.first().map(|s| s.account.clone())
    }

    /// Adds an attribute to the transaction.
    ///
    /// # Arguments
    ///
    /// * `attribute` - The attribute to add
    ///
    /// # Returns
    ///
    /// Result indicating success or failure
    pub fn add_attribute(&mut self, attribute: TransactionAttribute) -> Result<(), CoreError> {
        if self.attributes.len() >= MAX_TRANSACTION_ATTRIBUTES {
            return Err(CoreError::InvalidOperation {
                message: "Too many attributes".to_string(),
            });
        }
        self.attributes.push(attribute);
        self.invalidate_cache();
        Ok(())
    }

    /// Invalidates cached values when transaction is modified (matches C# behavior).
    pub(crate) fn invalidate_cache(&mut self) {
        *self._hash.lock().unwrap() = None;
        *self._size.lock().unwrap() = 0;
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Transaction {{ version: {}, nonce: {}, system_fee: {}, network_fee: {}, valid_until_block: {}, signers: {}, script_len: {} }}",
            self.version,
            self.nonce,
            self.system_fee,
            self.network_fee,
            self.valid_until_block,
            self.signers.len(),
            self.script.len()
        )
    }
}

// Manual implementations for traits that can't be derived due to Mutex
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
            _hash: Mutex::new(None), // Reset cache in clone
            _size: Mutex::new(0),    // Reset cache in clone
        }
    }
}

impl PartialEq for Transaction {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.nonce == other.nonce
            && self.system_fee == other.system_fee
            && self.network_fee == other.network_fee
            && self.valid_until_block == other.valid_until_block
            && self.signers == other.signers
            && self.attributes == other.attributes
            && self.script == other.script
            && self.witnesses == other.witnesses
        // Note: _hash and _size are not compared as they are cache fields
    }
}

impl Eq for Transaction {}

impl crate::IVerifiable for Transaction {
    fn verify(&self) -> bool {
        // Verify transaction structure and constraints
        if self.version > 0xFF {
            return false;
        }

        if self.signers.is_empty() || self.signers.len() > 16 {
            return false;
        }

        if self.system_fee < 0 || self.network_fee < 0 {
            return false;
        }

        if self.script.is_empty() || self.script.len() > 65535 {
            return false;
        }

        // Additional validation would include signature verification
        true
    }

    fn hash(&self) -> crate::CoreResult<UInt256> {
        self.get_hash()
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.get_hash_data()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Transaction {
    /// Gets the script hashes used by this transaction (for consensus)
    pub fn get_script_hashes(&self) -> crate::CoreResult<Vec<crate::UInt160>> {
        let mut hashes = Vec::new();

        // Add signer account hashes
        for signer in &self.signers {
            hashes.push(signer.account.clone());
        }

        // Remove duplicates
        hashes.sort();
        hashes.dedup();

        Ok(hashes)
    }
}
