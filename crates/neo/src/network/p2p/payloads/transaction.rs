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
    signer::Signer, transaction_attribute::TransactionAttribute,
    transaction_attribute_type::TransactionAttributeType, witness::Witness,
};
use crate::cryptography::crypto_utils::Secp256r1Crypto;
use crate::hardfork::{is_hardfork_enabled, Hardfork};
use crate::neo_crypto::sha256;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::helper::Helper as P2PHelper;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::{ContractManagement, LedgerContract, PolicyContract};
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::{ContractBasicMethod, ContractParameterType, IInteroperable};
use crate::wallets::helper::Helper as WalletHelper;
use crate::{ledger::VerifyResult, CoreResult, UInt160, UInt256};
use base64::{engine::general_purpose, Engine as _};
use neo_vm::{op_code::OpCode, StackItem};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::hash::{Hash as StdHash, Hasher};
use std::sync::{Arc, Mutex};

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

    /// Adds a signer to the transaction.
    pub fn add_signer(&mut self, signer: Signer) {
        self.signers.push(signer);
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
        *self._hash.lock().unwrap() = None;
        *self._size.lock().unwrap() = None;
    }

    /// Adds a single attribute to the transaction.
    pub fn add_attribute(&mut self, attribute: TransactionAttribute) {
        self.attributes.push(attribute);
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

    /// Adds a witness to the transaction.
    pub fn add_witness(&mut self, witness: Witness) {
        self.witnesses.push(witness);
        *self._size.lock().unwrap() = None;
    }

    /// Returns the transaction hash (C# compatibility helper).
    pub fn get_hash(&self) -> UInt256 {
        self.hash()
    }

    /// Returns the unsigned serialization used for hashing.
    pub fn get_hash_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        Self::serialize_unsigned(self, &mut writer)
            .expect("failed to serialize transaction unsigned data");
        writer.into_bytes()
    }

    /// Serializes the transaction into bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        <Self as Serializable>::serialize(self, &mut writer).expect("transaction serialization");
        writer.into_bytes()
    }

    /// Deserializes a transaction from bytes.
    pub fn from_bytes(bytes: &[u8]) -> IoResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        <Self as Serializable>::deserialize(&mut reader)
    }

    /// Gets the hash of the transaction.
    pub fn hash(&self) -> UInt256 {
        let mut hash_guard = self._hash.lock().unwrap();
        if let Some(hash) = *hash_guard {
            return hash;
        }

        // Calculate hash from serialized unsigned data
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("transaction serialization should not fail");
        let hash = UInt256::from(sha256(&writer.into_bytes()));
        *hash_guard = Some(hash);
        hash
    }

    /// Gets the sender (first signer) of the transaction.
    /// The sender will pay the fees of the transaction.
    pub fn sender(&self) -> Option<UInt160> {
        self.signers.first().map(|s| s.account)
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

    /// Gets the first attribute of the specified type.
    pub fn get_attribute(
        &self,
        attr_type: TransactionAttributeType,
    ) -> Option<&TransactionAttribute> {
        self.attributes
            .iter()
            .find(|attr| attr.get_type() == attr_type)
    }

    /// Gets all attributes of the specified type.
    pub fn get_attributes(
        &self,
        attr_type: TransactionAttributeType,
    ) -> Vec<&TransactionAttribute> {
        self.attributes
            .iter()
            .filter(|attr| attr.get_type() == attr_type)
            .collect()
    }

    /// Serialize without witnesses.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.version)?;
        writer.write_u32(self.nonce)?;
        writer.write_i64(self.system_fee)?;
        writer.write_i64(self.network_fee)?;
        writer.write_u32(self.valid_until_block)?;

        // Write signers
        writer.write_var_uint(self.signers.len() as u64)?;
        for signer in &self.signers {
            writer.write_serializable(signer)?;
        }

        // Write attributes
        writer.write_var_uint(self.attributes.len() as u64)?;
        for attr in &self.attributes {
            writer.write_serializable(attr)?;
        }

        if self.script.len() > u16::MAX as usize {
            return Err(IoError::invalid_data(
                "Transaction script exceeds maximum length",
            ));
        }
        writer.write_var_bytes(&self.script)?;

        Ok(())
    }

    /// Deserialize unsigned transaction data.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u8()?;
        if version > 0 {
            return Err(IoError::invalid_data("Invalid transaction version"));
        }

        let nonce = reader.read_u32()?;

        let system_fee = reader.read_i64()?;
        if system_fee < 0 {
            return Err(IoError::invalid_data("Invalid system fee"));
        }

        let network_fee = reader.read_i64()?;
        if network_fee < 0 {
            return Err(IoError::invalid_data("Invalid network fee"));
        }

        if system_fee + network_fee < system_fee {
            return Err(IoError::invalid_data("Invalid combined fee"));
        }

        let valid_until_block = reader.read_u32()?;

        // Read signers
        let signers = Self::deserialize_signers(reader, MAX_TRANSACTION_ATTRIBUTES)?;

        // Read attributes
        let attributes =
            Self::deserialize_attributes(reader, MAX_TRANSACTION_ATTRIBUTES - signers.len())?;

        // Read script
        let script = reader.read_var_bytes(u16::MAX as usize)?;
        if script.is_empty() {
            return Err(IoError::invalid_data("Script length cannot be zero"));
        }

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
        })
    }

    fn deserialize_signers(reader: &mut MemoryReader, max_count: usize) -> IoResult<Vec<Signer>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        if count == 0 {
            return Err(IoError::invalid_data("Signer count cannot be zero"));
        }
        if count > max_count {
            return Err(IoError::invalid_data("Too many signers"));
        }

        let mut signers = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let signer = <Signer as Serializable>::deserialize(reader)?;
            if !hashset.insert(signer.account) {
                return Err(IoError::invalid_data("Duplicate signer"));
            }
            signers.push(signer);
        }

        Ok(signers)
    }

    fn deserialize_attributes(
        reader: &mut MemoryReader,
        max_count: usize,
    ) -> IoResult<Vec<TransactionAttribute>> {
        let count = reader.read_var_int(max_count as u64)? as usize;
        if count > max_count {
            return Err(IoError::invalid_data("Too many attributes"));
        }

        let mut attributes = Vec::with_capacity(count);
        let mut hashset = HashSet::new();

        for _ in 0..count {
            let attribute = <TransactionAttribute as Serializable>::deserialize(reader)?;
            if !attribute.allow_multiple() && !hashset.insert(attribute.get_type()) {
                return Err(IoError::invalid_data("Duplicate attribute"));
            }
            attributes.push(attribute);
        }

        Ok(attributes)
    }

    /// Converts the transaction to a JSON object.
    pub fn to_json(&self, settings: &ProtocolSettings) -> serde_json::Value {
        let mut json = serde_json::Map::new();

        json.insert(
            "hash".to_string(),
            serde_json::json!(self.hash().to_string()),
        );
        json.insert("size".to_string(), serde_json::json!(self.size()));
        json.insert("version".to_string(), serde_json::json!(self.version));
        json.insert("nonce".to_string(), serde_json::json!(self.nonce));

        let sender_value = self
            .sender()
            .map(|account| WalletHelper::to_address(&account, settings.address_version));
        json.insert(
            "sender".to_string(),
            sender_value
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );

        json.insert(
            "sysfee".to_string(),
            serde_json::json!(self.system_fee.to_string()),
        );
        json.insert(
            "netfee".to_string(),
            serde_json::json!(self.network_fee.to_string()),
        );
        json.insert(
            "validuntilblock".to_string(),
            serde_json::json!(self.valid_until_block),
        );

        let signers_json: Vec<_> = self.signers.iter().map(|s| s.to_json()).collect();
        json.insert(
            "signers".to_string(),
            serde_json::Value::Array(signers_json),
        );

        let attributes_json: Vec<_> = self.attributes.iter().map(|a| a.to_json()).collect();
        json.insert(
            "attributes".to_string(),
            serde_json::Value::Array(attributes_json),
        );

        json.insert(
            "script".to_string(),
            serde_json::json!(general_purpose::STANDARD.encode(&self.script)),
        );

        let witnesses_json: Vec<_> = self.witnesses.iter().map(|w| w.to_json()).collect();
        json.insert(
            "witnesses".to_string(),
            serde_json::Value::Array(witnesses_json),
        );

        serde_json::Value::Object(json)
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
        let ledger = LedgerContract::new();
        let policy = PolicyContract::new();
        let height = ledger.current_index(snapshot).unwrap_or(0);
        let max_increment = policy
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
            .unwrap_or(settings.max_valid_until_block_increment);
        if self.valid_until_block <= height || self.valid_until_block > height + max_increment {
            return VerifyResult::Expired;
        }

        let hashes = self.get_script_hashes_for_verifying(snapshot);
        for hash in &hashes {
            if policy.is_blocked_snapshot(snapshot, hash).unwrap_or(false) {
                return VerifyResult::PolicyFail;
            }
        }

        if let Some(ctx) = context {
            if !ctx.check_transaction(self, conflicts_list.iter(), snapshot) {
                return VerifyResult::InsufficientFunds;
            }
        }

        let mut attributes_fee = 0i64;
        for attribute in &self.attributes {
            if attribute.get_type() == TransactionAttributeType::NotaryAssisted
                && !is_hardfork_enabled(Hardfork::HfEchidna, height)
            {
                return VerifyResult::InvalidAttribute;
            }
            if !attribute.verify(settings, snapshot, self) {
                return VerifyResult::InvalidAttribute;
            }
            attributes_fee += attribute.calculate_network_fee(snapshot, self);
        }

        let fee_per_byte = policy
            .get_fee_per_byte_snapshot(snapshot)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);
        let mut net_fee_datoshi =
            self.network_fee - (self.size() as i64 * fee_per_byte) - attributes_fee;

        if net_fee_datoshi < 0 {
            return VerifyResult::InsufficientFunds;
        }

        let max_verification_gas = Helper::MAX_VERIFICATION_GAS;
        if net_fee_datoshi > max_verification_gas {
            net_fee_datoshi = max_verification_gas;
        }

        let exec_fee_factor = policy
            .get_exec_fee_factor_snapshot(snapshot)
            .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR)
            as i64;

        let sign_data = self.get_sign_data(settings.network);

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            if let Some(public_key) =
                Self::parse_single_signature_contract(&witness.verification_script)
            {
                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                let Some(signature) =
                    Self::parse_single_signature_invocation(&witness.invocation_script)
                else {
                    return VerifyResult::Invalid;
                };

                let mut signature_bytes = [0u8; 64];
                signature_bytes.copy_from_slice(signature);

                let verified =
                    match Secp256r1Crypto::verify(&sign_data, &signature_bytes, public_key) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                if !verified {
                    return VerifyResult::InvalidSignature;
                }

                net_fee_datoshi -= exec_fee_factor * Helper::signature_contract_cost();
            } else if let Some((m, public_keys)) =
                Helper::parse_multi_sig_contract(&witness.verification_script)
            {
                let Some(signatures) =
                    Helper::parse_multi_sig_invocation(&witness.invocation_script, m)
                else {
                    return VerifyResult::Invalid;
                };

                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                if public_keys.is_empty() || signatures.len() != m {
                    return VerifyResult::Invalid;
                }

                let total_keys = public_keys.len();
                let mut sig_index = 0usize;
                let mut key_index = 0usize;

                while sig_index < m && key_index < total_keys {
                    let signature = &signatures[sig_index];
                    if signature.len() != 64 {
                        return VerifyResult::Invalid;
                    }

                    let mut signature_bytes = [0u8; 64];
                    signature_bytes.copy_from_slice(signature);

                    let verified = match Secp256r1Crypto::verify(
                        &sign_data,
                        &signature_bytes,
                        &public_keys[key_index],
                    ) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                    if verified {
                        sig_index += 1;
                    }

                    key_index += 1;

                    if m.saturating_sub(sig_index) > total_keys.saturating_sub(key_index) {
                        return VerifyResult::InvalidSignature;
                    }
                }

                if sig_index != m {
                    return VerifyResult::InvalidSignature;
                }

                let n = public_keys.len() as i32;
                net_fee_datoshi -=
                    exec_fee_factor * Helper::multi_signature_contract_cost(m as i32, n);
            } else {
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

        if neo_vm::Script::new(self.script.clone(), true).is_err() {
            return VerifyResult::InvalidScript;
        }

        let hashes = self.get_script_hashes_for_verifying(&DataCache::new(true));
        if hashes.len() != self.witnesses.len() {
            return VerifyResult::Invalid;
        }

        let sign_data = self.get_sign_data(settings.network);

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            if witness.script_hash() != *hash {
                return VerifyResult::Invalid;
            }

            if Helper::is_signature_contract(&witness.verification_script) {
                if witness.verification_script.len() < 35 {
                    return VerifyResult::Invalid;
                }

                let Some(signature) =
                    Self::parse_single_signature_invocation(&witness.invocation_script)
                else {
                    return VerifyResult::Invalid;
                };

                let mut signature_bytes = [0u8; 64];
                signature_bytes.copy_from_slice(signature);

                let pubkey = &witness.verification_script[2..35];
                let verified = match Secp256r1Crypto::verify(&sign_data, &signature_bytes, pubkey) {
                    Ok(result) => result,
                    Err(_) => return VerifyResult::Invalid,
                };

                if !verified {
                    return VerifyResult::InvalidSignature;
                }
            } else if let Some((m, public_keys)) =
                Helper::parse_multi_sig_contract(&witness.verification_script)
            {
                let Some(signatures) =
                    Helper::parse_multi_sig_invocation(&witness.invocation_script, m)
                else {
                    return VerifyResult::Invalid;
                };

                if public_keys.is_empty() || signatures.len() != m {
                    return VerifyResult::Invalid;
                }

                let total_keys = public_keys.len();
                let mut sig_index = 0usize;
                let mut key_index = 0usize;

                while sig_index < m && key_index < total_keys {
                    let signature = &signatures[sig_index];
                    if signature.len() != 64 {
                        return VerifyResult::Invalid;
                    }

                    let mut signature_bytes = [0u8; 64];
                    signature_bytes.copy_from_slice(signature);

                    let verified = match Secp256r1Crypto::verify(
                        &sign_data,
                        &signature_bytes,
                        &public_keys[key_index],
                    ) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                    if verified {
                        sig_index += 1;
                    }

                    key_index += 1;

                    if m.saturating_sub(sig_index) > total_keys.saturating_sub(key_index) {
                        return VerifyResult::InvalidSignature;
                    }
                }

                if sig_index != m {
                    return VerifyResult::InvalidSignature;
                }
            }
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
        *fee = 0;

        if gas < 0 {
            return false;
        }

        if witness.script_hash() != *hash {
            return false;
        }

        let verification_gas = gas.min(Helper::MAX_VERIFICATION_GAS);

        if neo_vm::Script::new(witness.invocation_script.clone(), true).is_err() {
            return false;
        }

        let container: Arc<dyn crate::IVerifiable> = Arc::new(self.clone());
        let snapshot_clone = Arc::new(snapshot.clone());

        let mut engine = match ApplicationEngine::new(
            TriggerType::Verification,
            Some(container),
            snapshot_clone,
            None,
            settings.clone(),
            verification_gas,
            None,
        ) {
            Ok(engine) => engine,
            Err(_) => return false,
        };

        let verification_script = witness.verification_script.clone();

        if verification_script.is_empty() {
            let contract = match ContractManagement::get_contract_from_snapshot(snapshot, hash) {
                Ok(Some(contract)) => contract,
                _ => return false,
            };

            let mut abi = contract.manifest.abi.clone();
            let method = match abi.get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            ) {
                Some(descriptor) => descriptor.clone(),
                None => return false,
            };

            if method.return_type != ContractParameterType::Boolean {
                return false;
            }

            if engine
                .load_contract_method(contract, method, CallFlags::READ_ONLY)
                .is_err()
            {
                return false;
            }
        } else {
            if neo_vm::Script::new(verification_script.clone(), true).is_err() {
                return false;
            }
            if engine
                .load_script(verification_script, CallFlags::READ_ONLY, Some(*hash))
                .is_err()
            {
                return false;
            }
        }

        if engine
            .load_script(witness.invocation_script.clone(), CallFlags::NONE, None)
            .is_err()
        {
            return false;
        }

        if engine.execute().is_err() {
            return false;
        }

        if engine.result_stack().len() != 1 {
            return false;
        }

        let Ok(result_item) = engine.result_stack().peek(0) else {
            return false;
        };

        match result_item.get_boolean() {
            Ok(true) => {
                *fee = engine.fee_consumed();
                true
            }
            _ => false,
        }
    }

    /// Get signature data for the transaction.
    fn get_sign_data(&self, network: u32) -> Vec<u8> {
        P2PHelper::get_sign_data_vec(self, network)
            .expect("transaction hash should always be available for signing")
    }

    fn parse_single_signature_contract(script: &[u8]) -> Option<&[u8]> {
        if script.len() != 35 {
            return None;
        }
        if script[0] != OpCode::PUSHDATA1 as u8 || script[1] != 0x21 {
            return None;
        }
        if script[34] != OpCode::SYSCALL as u8 {
            return None;
        }
        Some(&script[2..34])
    }

    fn parse_single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
        if invocation.len() != 66 {
            return None;
        }
        if invocation[0] != OpCode::PUSHDATA1 as u8 || invocation[1] != 0x40 {
            return None;
        }
        Some(&invocation[2..66])
    }
}

impl IInventory for Transaction {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Transaction
    }

    fn hash(&mut self) -> UInt256 {
        Transaction::hash(self)
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
            + get_var_size(self.signers.len() as u64)
            + self.signers.iter().map(|s| s.size()).sum::<usize>()
            + get_var_size(self.attributes.len() as u64)
            + self.attributes.iter().map(|a| a.size()).sum::<usize>()
            + get_var_size(self.script.len() as u64)
            + self.script.len()
            + get_var_size(self.witnesses.len() as u64)
            + self.witnesses.iter().map(|w| w.size()).sum::<usize>();

        *size_guard = Some(size);
        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        if self.witnesses.len() != self.signers.len() {
            return Err(IoError::invalid_data(
                "Witness count must match signer count",
            ));
        }
        writer.write_var_uint(self.witnesses.len() as u64)?;
        for witness in &self.witnesses {
            writer.write_serializable(witness)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut tx = Self::deserialize_unsigned(reader)?;

        let witness_count = reader.read_var_int(tx.signers.len() as u64)? as usize;
        if witness_count != tx.signers.len() {
            return Err(IoError::invalid_data("Witness count mismatch"));
        }

        let mut witnesses = Vec::with_capacity(witness_count);
        for _ in 0..witness_count {
            witnesses.push(<Witness as Serializable>::deserialize(reader)?);
        }
        tx.witnesses = witnesses;

        *tx._size.lock().unwrap() = None;
        Ok(tx)
    }
}

impl crate::IVerifiable for Transaction {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> CoreResult<UInt256> {
        Ok(Transaction::hash(self))
    }

    fn get_hash_data(&self) -> Vec<u8> {
        Transaction::get_hash_data(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IInteroperable for Transaction {
    fn from_stack_item(&mut self, _stack_item: StackItem) {
        panic!("NotSupportedException: Transaction::from_stack_item is not supported");
    }

    fn to_stack_item(&self) -> StackItem {
        if self.signers.is_empty() {
            panic!("ArgumentException: Sender is not specified in the transaction.");
        }

        let sender = self
            .sender()
            .expect("signers.is_empty() already validated")
            .to_bytes();

        StackItem::from_array(vec![
            StackItem::from_byte_string(self.hash().to_bytes()),
            StackItem::from_int(self.version as i64),
            StackItem::from_int(self.nonce),
            StackItem::from_byte_string(sender),
            StackItem::from_int(self.system_fee),
            StackItem::from_int(self.network_fee),
            StackItem::from_int(self.valid_until_block),
            StackItem::from_byte_string(self.script.clone()),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

// Eq and PartialEq are already derived

impl std::hash::Hash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash_bytes = self.hash().to_bytes();
        StdHash::hash(&hash_bytes, state);
    }
}
