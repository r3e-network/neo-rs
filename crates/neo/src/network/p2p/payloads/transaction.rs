// Copyright (C) 2015-2025 The Neo Project.
//
// transaction.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    i_inventory::IInventory, i_verifiable::IVerifiable, inventory_type::InventoryType,
    signer::Signer, transaction_attribute::TransactionAttribute, witness::Witness,
};
use crate::neo_crypto::sha256;
use crate::neo_io::{MemoryReader, Serializable};
use crate::neo_system::ProtocolSettings;
use crate::persistence::DataCache;
use crate::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::Mutex;

/// The maximum size of a transaction.
pub const MAX_TRANSACTION_SIZE: usize = 102400;

/// The maximum number of attributes that can be contained within a transaction.
pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;

/// The size of a transaction header.
pub const HEADER_SIZE: usize = 1 + 4 + 8 + 8 + 4; // Version + Nonce + SystemFee + NetworkFee + ValidUntilBlock

/// Result of transaction verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Succeed,
    AlreadyExists,
    MemPoolFull,
    AlreadyInPool,
    InsufficientFunds,
    UnableToVerify,
    Invalid,
    InvalidSignature,
    OverSize,
    Expired,
    InvalidScript,
    InvalidAttribute,
    PolicyFail,
    Unknown,
}

/// Represents a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// Version of the transaction format.
    version: u8,

    /// Random number to avoid hash collision.
    nonce: u32,

    /// System fee in datoshi (1 datoshi = 1e-8 GAS).
    system_fee: i64,

    /// Network fee in datoshi (1 datoshi = 1e-8 GAS).
    network_fee: i64,

    /// Block height when transaction expires.
    valid_until_block: u32,

    /// Signers of the transaction.
    signers: Vec<Signer>,

    /// Attributes of the transaction.
    attributes: Vec<TransactionAttribute>,

    /// Script to be executed.
    script: Vec<u8>,

    /// Witnesses for verification.
    witnesses: Vec<Witness>,

    #[serde(skip)]
    _hash: Mutex<Option<UInt256>>,

    #[serde(skip)]
    _size: Mutex<Option<usize>>,

    #[serde(skip)]
    _attributes_cache: Mutex<Option<HashMap<String, Vec<TransactionAttribute>>>>,
}

impl Transaction {
    /// Creates a new transaction.
    pub fn new() -> Self {
        Self {
            version: 0,
            nonce: rand::random(),
            system_fee: 0,
            network_fee: 0,
            valid_until_block: 0,
            signers: Vec::new(),
            attributes: Vec::new(),
            script: Vec::new(),
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
            _attributes_cache: Mutex::new(None),
        }
    }

    /// Gets the version of the transaction.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Sets the version of the transaction.
    pub fn set_version(&mut self, version: u8) {
        self.version = version;
        *self._hash.lock().unwrap() = None;
    }

    /// Gets the nonce of the transaction.
    pub fn nonce(&self) -> u32 {
        self.nonce
    }

    /// Sets the nonce of the transaction.
    pub fn set_nonce(&mut self, nonce: u32) {
        self.nonce = nonce;
        *self._hash.lock().unwrap() = None;
    }

    /// Gets the system fee of the transaction.
    pub fn system_fee(&self) -> i64 {
        self.system_fee
    }

    /// Sets the system fee of the transaction.
    pub fn set_system_fee(&mut self, system_fee: i64) {
        self.system_fee = system_fee;
        *self._hash.lock().unwrap() = None;
    }

    /// Gets the network fee of the transaction.
    pub fn network_fee(&self) -> i64 {
        self.network_fee
    }

    /// Sets the network fee of the transaction.
    pub fn set_network_fee(&mut self, network_fee: i64) {
        self.network_fee = network_fee;
        *self._hash.lock().unwrap() = None;
    }

    /// Gets the valid until block of the transaction.
    pub fn valid_until_block(&self) -> u32 {
        self.valid_until_block
    }

    /// Sets the valid until block of the transaction.
    pub fn set_valid_until_block(&mut self, valid_until_block: u32) {
        self.valid_until_block = valid_until_block;
        *self._hash.lock().unwrap() = None;
    }

    /// Gets the signers of the transaction.
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// Sets the signers of the transaction.
    pub fn set_signers(&mut self, signers: Vec<Signer>) {
        self.signers = signers;
        *self._hash.lock().unwrap() = None;
        *self._size.lock().unwrap() = None;
    }

    /// Gets the attributes of the transaction.
    pub fn attributes(&self) -> &[TransactionAttribute] {
        &self.attributes
    }

    /// Sets the attributes of the transaction.
    pub fn set_attributes(&mut self, attributes: Vec<TransactionAttribute>) {
        self.attributes = attributes;
        *self._attributes_cache.lock().unwrap() = None;
        *self._hash.lock().unwrap() = None;
        *self._size.lock().unwrap() = None;
    }

    /// Gets the script of the transaction.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Sets the script of the transaction.
    pub fn set_script(&mut self, script: Vec<u8>) {
        self.script = script;
        *self._hash.lock().unwrap() = None;
        *self._size.lock().unwrap() = None;
    }

    /// Gets the witnesses of the transaction.
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }

    /// Sets the witnesses of the transaction.
    pub fn set_witnesses(&mut self, witnesses: Vec<Witness>) {
        self.witnesses = witnesses;
        *self._size.lock().unwrap() = None;
    }

    /// Gets the hash of the transaction.
    pub fn hash(&self) -> UInt256 {
        let mut hash_guard = self._hash.lock().unwrap();
        if let Some(hash) = *hash_guard {
            return hash;
        }

        // Calculate hash from serialized unsigned data
        let mut data = Vec::new();
        self.serialize_unsigned(&mut data).unwrap();
        let hash = UInt256::from(sha256(&data));
        *hash_guard = Some(hash);
        hash
    }

    /// Gets the sender (first signer) of the transaction.
    /// The sender will pay the fees of the transaction.
    pub fn sender(&self) -> UInt160 {
        self.signers[0].account
    }

    /// Gets the fee per byte.
    pub fn fee_per_byte(&self) -> i64 {
        let size = self.size();
        if size == 0 {
            0
        } else {
            self.network_fee / size as i64
        }
    }

    /// Gets the attribute of the specified type.
    pub fn get_attribute<T: 'static>(&self) -> Option<&TransactionAttribute> {
        self.get_attributes::<T>().first().map(|a| *a)
    }

    /// Gets all attributes of the specified type.
    pub fn get_attributes<T: 'static>(&self) -> Vec<&TransactionAttribute> {
        let mut cache_guard = self._attributes_cache.lock().unwrap();

        if cache_guard.is_none() {
            let mut cache = HashMap::new();
            for attr in &self.attributes {
                let type_name = std::any::type_name::<T>().to_string();
                cache
                    .entry(type_name.clone())
                    .or_insert_with(Vec::new)
                    .push(attr.clone());
            }
            *cache_guard = Some(cache);
        }

        if let Some(cache) = cache_guard.as_ref() {
            let type_name = std::any::type_name::<T>().to_string();
            if let Some(attrs) = cache.get(&type_name) {
                return attrs.iter().collect();
            }
        }

        Vec::new()
    }

    /// Serialize without witnesses.
    pub fn serialize_unsigned(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&[self.version])?;
        writer.write_all(&self.nonce.to_le_bytes())?;
        writer.write_all(&self.system_fee.to_le_bytes())?;
        writer.write_all(&self.network_fee.to_le_bytes())?;
        writer.write_all(&self.valid_until_block.to_le_bytes())?;

        // Write signers
        writer.write_all(&[self.signers.len() as u8])?;
        for signer in &self.signers {
            signer.serialize(writer)?;
        }

        // Write attributes
        writer.write_all(&[self.attributes.len() as u8])?;
        for attr in &self.attributes {
            attr.serialize(writer)?;
        }

        // Write script
        writer.write_all(&(self.script.len() as u16).to_le_bytes())?;
        writer.write_all(&self.script)?;

        Ok(())
    }

    /// Deserialize unsigned transaction data.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> Result<Self, String> {
        let version = reader.read_u8().map_err(|e| e.to_string())?;
        if version > 0 {
            return Err(format!("Invalid version: {}", version));
        }

        let nonce = reader.read_u32().map_err(|e| e.to_string())?;

        let system_fee = reader.read_i64().map_err(|e| e.to_string())?;
        if system_fee < 0 {
            return Err(format!("Invalid system fee: {}", system_fee));
        }

        let network_fee = reader.read_i64().map_err(|e| e.to_string())?;
        if network_fee < 0 {
            return Err(format!("Invalid network fee: {}", network_fee));
        }

        if system_fee + network_fee < system_fee {
            return Err(format!(
                "Invalid fee: {} + {} < {}",
                system_fee, network_fee, system_fee
            ));
        }

        let valid_until_block = reader.read_u32().map_err(|e| e.to_string())?;

        // Read signers
        let signers = Self::deserialize_signers(reader, MAX_TRANSACTION_ATTRIBUTES)?;

        // Read attributes
        let attributes =
            Self::deserialize_attributes(reader, MAX_TRANSACTION_ATTRIBUTES - signers.len())?;

        // Read script
        let script_length = reader.read_var_int(65535).map_err(|e| e.to_string())?;
        if script_length == 0 {
            return Err("Script length cannot be zero".to_string());
        }
        let script = reader
            .read_bytes(script_length as usize)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            version,
            nonce,
            system_fee,
            network_fee,
            valid_until_block,
            signers,
            attributes,
            script,
            witnesses: Vec::new(),
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
            _attributes_cache: Mutex::new(None),
        })
    }

    fn deserialize_signers(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> Result<Vec<Signer>, String> {
        let count = reader.read_var_int(256).map_err(|e| e.to_string())? as usize;
        if count == 0 {
            return Err("Signer count cannot be zero".to_string());
        }
        if count > max_count {
            return Err("Too many signers".to_string());
        }

        let mut signers = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let signer = Signer::deserialize(reader)?;
            if !hashset.insert(signer.account) {
                return Err("Duplicate signer".to_string());
            }
            signers.push(signer);
        }

        Ok(signers)
    }

    fn deserialize_attributes(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> Result<Vec<TransactionAttribute>, String> {
        let count = reader.read_var_int(256).map_err(|e| e.to_string())? as usize;
        if count > max_count {
            return Err("Too many attributes".to_string());
        }

        let mut attributes = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let attribute = TransactionAttribute::deserialize(reader)?;
            if !attribute.allow_multiple() && !hashset.insert(attribute.get_type()) {
                return Err("Duplicate attribute".to_string());
            }
            attributes.push(attribute);
        }

        Ok(attributes)
    }

    /// Converts the transaction to a JSON object.
    pub fn to_json(&self, settings: &ProtocolSettings) -> serde_json::Value {
        // TODO: Implement to_address and to_json methods for complete JSON conversion
        serde_json::json!({
            "hash": self.hash().to_string(),
            "size": self.size(),
            "version": self.version,
            "nonce": self.nonce,
            "sender": format!("0x{}", hex::encode(self.sender().to_array())),
            "sysfee": self.system_fee.to_string(),
            "netfee": self.network_fee.to_string(),
            "validuntilblock": self.valid_until_block,
            "signers": self.signers.len(), // TODO: map to JSON when to_json is available
            "attributes": self.attributes.len(), // TODO: map to JSON when to_json is available
            "script": hex::encode(&self.script),
            "witnesses": self.witnesses.len(), // TODO: map to JSON when to_json is available
        })
    }

    /// Verifies the transaction.
    pub fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        let result = self.verify_state_independent(settings);
        if result != VerifyResult::Succeed {
            return result;
        }
        self.verify_state_dependent(settings, snapshot, context, conflicts_list)
    }

    /// Verifies the state-dependent part of the transaction.
    pub fn verify_state_dependent(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        // TODO: Import NativeContract when available
        // use crate::smart_contract::native::NativeContract;

        // TODO: Get current block height from ledger
        let height = 0u32; // NativeContract::ledger().current_index(snapshot);
        if self.valid_until_block <= height || self.valid_until_block > height + 2102400 {
            // Default max increment
            return VerifyResult::Expired;
        }

        let hashes = self.get_script_hashes_for_verifying(snapshot);
        // TODO: Check blocked accounts when Policy contract is available
        // for hash in &hashes {
        //     if NativeContract::policy().is_blocked(snapshot, hash) {
        //         return VerifyResult::PolicyFail;
        //     }
        // }

        if let Some(ctx) = context {
            // TODO: Fix when check_transaction is properly implemented
            // if !ctx.check_transaction(self, conflicts_list, snapshot, settings) {
            //     return VerifyResult::InsufficientFunds;
            // }
        }

        let mut attributes_fee = 0i64;
        // TODO: Implement attribute verification when methods are available
        // for attribute in &self.attributes {
        //     if attribute.get_type() == TransactionAttributeType::NotaryAssisted &&
        //        !settings.is_hardfork_enabled(Hardfork::HF_Echidna, height) {
        //         return VerifyResult::InvalidAttribute;
        //     }
        //     if !attribute.verify(snapshot, self) {
        //         return VerifyResult::InvalidAttribute;
        //     }
        //     attributes_fee += attribute.calculate_network_fee(snapshot, self);
        // }

        // TODO: Get fee per byte from Policy contract
        let fee_per_byte = 1000i64; // Default fee per byte
        let mut net_fee_datoshi =
            self.network_fee - (self.size() as i64 * fee_per_byte) - attributes_fee;

        if net_fee_datoshi < 0 {
            return VerifyResult::InsufficientFunds;
        }

        const MAX_VERIFICATION_GAS: i64 = 1_00000000;
        if net_fee_datoshi > MAX_VERIFICATION_GAS {
            net_fee_datoshi = MAX_VERIFICATION_GAS;
        }

        // TODO: Get exec fee factor from Policy contract
        let exec_fee_factor = 30u32; // Default exec fee factor

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            // TODO: Implement witness verification with Helper methods
            // if Helper::is_signature_contract(&witness.verification_script) {
            //     if let Some(_sig) = Helper::is_single_signature_invocation_script(&witness.invocation_script) {
            //         net_fee_datoshi -= exec_fee_factor as i64 * Helper::signature_contract_cost();
            //     }
            // } else if let Some((m, n)) = Helper::is_multi_sig_contract(&witness.verification_script) {
            //     if Helper::is_multi_signature_invocation_script(m, &witness.invocation_script).is_some() {
            //         net_fee_datoshi -= exec_fee_factor as i64 * Helper::multi_signature_contract_cost(m, n);
            //     }
            // } else {
            {
                let mut fee = 0i64;
                if !self.verify_witness(
                    settings,
                    snapshot,
                    hash,
                    witness,
                    net_fee_datoshi,
                    &mut fee,
                ) {
                    return VerifyResult::Invalid;
                }
                net_fee_datoshi -= fee;
            }

            if net_fee_datoshi < 0 {
                return VerifyResult::InsufficientFunds;
            }
        }

        VerifyResult::Succeed
    }

    /// Verifies the state-independent part of the transaction.
    pub fn verify_state_independent(&self, settings: &ProtocolSettings) -> VerifyResult {
        if self.size() > MAX_TRANSACTION_SIZE {
            return VerifyResult::OverSize;
        }

        // TODO: Verify script validity when VM Script is available
        // match crate::vm::Script::new(&self.script, true) {
        //     Ok(_) => {},
        //     Err(_) => return VerifyResult::InvalidScript,
        // }

        // TODO: Fix when get_script_hashes_for_verifying is properly implemented
        let hashes = self.signers.iter().map(|s| s.account).collect::<Vec<_>>();
        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            // TODO: Implement signature verification when Helper methods are available
            // if Helper::is_signature_contract(&witness.verification_script) {
            //     if let Some(signature) = Helper::is_single_signature_invocation_script(&witness.invocation_script) {
            //         if *hash != witness.script_hash() {
            //             return VerifyResult::Invalid;
            //         }
            //
            //         let pubkey = &witness.verification_script[2..35];
            //         let message = self.get_sign_data(settings.network);
            //
            //         if !neo_crypto::verify_signature(&message, &signature, pubkey, neo_crypto::ECCurve::Secp256r1) {
            //             return VerifyResult::InvalidSignature;
            //         }
            //     }
            // } else if let Some((m, points)) = Helper::is_multi_sig_contract_with_points(&witness.verification_script) {
            //     if let Some(signatures) = Helper::is_multi_signature_invocation_script(m, &witness.invocation_script) {
            //         if *hash != witness.script_hash() {
            //             return VerifyResult::Invalid;
            //         }
            //
            //         let n = points.len();
            //         let message = self.get_sign_data(settings.network);
            //
            //         let mut x = 0;
            //         let mut y = 0;
            //         while x < m && y < n {
            //             if neo_crypto::verify_signature(&message, &signatures[x], &points[y], neo_crypto::ECCurve::Secp256r1) {
            //                 x += 1;
            //             }
            //             y += 1;
            //
            //             if m - x > n - y {
            //                 return VerifyResult::InvalidSignature;
            //             }
            //         }
            //     }
            // }
        }

        VerifyResult::Succeed
    }

    /// Verify a single witness.
    fn verify_witness(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        hash: &UInt160,
        witness: &Witness,
        gas: i64,
        fee: &mut i64,
    ) -> bool {
        // TODO: Implement ApplicationEngine execution when available
        // use crate::smart_contract::ApplicationEngine;
        //
        // let mut engine = ApplicationEngine::create(
        //     TriggerType::Verification,
        //     self,
        //     snapshot,
        //     None,
        //     settings,
        //     gas,
        // );
        //
        // engine.load_script(witness.invocation_script.clone(), CallFlags::None);
        // engine.load_script(witness.verification_script.clone(), CallFlags::ReadOnly);
        //
        // if engine.execute() == VMState::FAULT {
        //     return false;
        // }
        //
        // if engine.result_stack.len() != 1 || !engine.result_stack[0].get_boolean() {
        //     return false;
        // }
        //
        // *fee = engine.gas_consumed;
        *fee = 0;
        true
    }

    /// Get signature data for the transaction.
    fn get_sign_data(&self, network: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"NEO3");
        data.extend_from_slice(&network.to_le_bytes());
        let hash_bytes = self.hash();
        data.extend_from_slice(hash_bytes.as_bytes());
        sha256(&data).to_vec()
    }
}

impl IInventory for Transaction {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::TX
    }

    fn hash(&mut self) -> UInt256 {
        self.hash()
    }
}

impl IVerifiable for Transaction {
    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.signers.iter().map(|s| s.account).collect()
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }
}

impl Serializable for Transaction {
    fn size(&self) -> usize {
        let mut size_guard = self._size.lock().unwrap();
        if let Some(size) = *size_guard {
            return size;
        }

        let size = HEADER_SIZE
            + 1
            + self.signers.iter().map(|s| s.size()).sum::<usize>()
            + 1
            + self.attributes.iter().map(|a| a.size()).sum::<usize>()
            + 2
            + self.script.len()
            + 1
            + self.witnesses.iter().map(|w| w.size()).sum::<usize>();

        *size_guard = Some(size);
        size
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.serialize_unsigned(writer)?;
        writer.write_all(&[self.witnesses.len() as u8])?;
        for witness in &self.witnesses {
            witness.serialize(writer)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let start_position = 0; // TODO: Get position when available
        let mut tx = Self::deserialize_unsigned(reader)?;

        let witness_count = reader.read_var_int(256).map_err(|e| e.to_string())? as usize;
        if witness_count != tx.signers.len() {
            return Err("Witness count mismatch".to_string());
        }

        let mut witnesses = Vec::with_capacity(witness_count);
        for _ in 0..witness_count {
            witnesses.push(Witness::deserialize(reader)?);
        }
        tx.witnesses = witnesses;

        // TODO: Calculate size properly when position is available
        *tx._size.lock().unwrap() = None;
        Ok(tx)
    }
}

// TODO: Implement InteropInterface when the trait is available
// impl InteropInterface for Transaction {
//     fn to_stack_item(&self, reference_counter: &ReferenceCounter) -> StackItem {
//         if self.signers.is_empty() {
//             panic!("Sender is not specified in the transaction");
//         }
//
//         StackItem::Array(vec![
//             // Computed properties
//             StackItem::ByteString(self.hash().to_array().to_vec()),
//
//             // Transaction properties
//             StackItem::Integer(self.version as i64),
//             StackItem::Integer(self.nonce as i64),
//             StackItem::ByteString(self.sender().to_array().to_vec()),
//             StackItem::Integer(self.system_fee),
//             StackItem::Integer(self.network_fee),
//             StackItem::Integer(self.valid_until_block as i64),
//             StackItem::ByteString(self.script.clone()),
//         ])
//     }
//
//     fn from_stack_item(_stack_item: &StackItem) -> Result<Self, String> {
//         Err("Not supported".to_string())
//     }
// }

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

// Eq and PartialEq are already derived

// TODO: Implement Hash trait when needed
// impl std::hash::Hash for Transaction {
//     fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         self.hash().hash(state);
//     }
// }
