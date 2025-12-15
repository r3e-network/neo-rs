// Copyright (C) 2015-2025 The Neo Project.
//
// header.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::witness::Witness;
use crate::error::CoreResult;
use crate::ledger::HeaderCache;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::{ContractManagement, LedgerContract};
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::{ContractBasicMethod, ContractParameterType};
use crate::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

/// Represents the header of a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    version: u32,
    prev_hash: UInt256,
    merkle_root: UInt256,
    timestamp: u64,
    nonce: u64,
    index: u32,
    primary_index: u8,
    next_consensus: UInt160,

    /// The witness of the block.
    pub witness: Witness,

    #[serde(skip)]
    _hash: Option<UInt256>,
}

const HEADER_VERIFY_GAS: i64 = 300_000_000;

impl Header {
    /// Creates a new header.
    pub fn new() -> Self {
        Self {
            version: 0,
            prev_hash: UInt256::default(),
            merkle_root: UInt256::default(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::default(),
            witness: Witness::new(),
            _hash: None,
        }
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Sets the version of the block.
    pub fn set_version(&mut self, value: u32) {
        self.version = value;
        self._hash = None;
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.prev_hash
    }

    /// Sets the hash of the previous block.
    pub fn set_prev_hash(&mut self, value: UInt256) {
        self.prev_hash = value;
        self._hash = None;
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        &self.merkle_root
    }

    /// Sets the merkle root of the transactions.
    pub fn set_merkle_root(&mut self, value: UInt256) {
        self.merkle_root = value;
        self._hash = None;
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Sets the timestamp of the block.
    pub fn set_timestamp(&mut self, value: u64) {
        self.timestamp = value;
        self._hash = None;
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Sets the nonce of the block.
    pub fn set_nonce(&mut self, value: u64) {
        self.nonce = value;
        self._hash = None;
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Sets the index of the block.
    pub fn set_index(&mut self, value: u32) {
        self.index = value;
        self._hash = None;
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.primary_index
    }

    /// Sets the primary index of the consensus node.
    pub fn set_primary_index(&mut self, value: u8) {
        self.primary_index = value;
        self._hash = None;
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        &self.next_consensus
    }

    /// Sets the next consensus address.
    pub fn set_next_consensus(&mut self, value: UInt160) {
        self.next_consensus = value;
        self._hash = None;
    }

    /// Gets the hash of the header.
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = self._hash {
            return hash;
        }

        // Calculate hash from serialized data
        let mut writer = BinaryWriter::new();
        if let Err(err) = self.serialize_unsigned(&mut writer) {
            tracing::error!("Header unsigned serialization failed: {err}");
            let hash = UInt256::zero();
            self._hash = Some(hash);
            return hash;
        }
        // Neo block hashes use single SHA256 over the unsigned header payload.
        let hash = UInt256::from(crate::neo_crypto::sha256(&writer.into_bytes()));
        self._hash = Some(hash);
        hash
    }

    /// Serialize without witness
    fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        let prev_hash = self.prev_hash.as_bytes();
        writer.write_bytes(&prev_hash)?;
        let merkle_root = self.merkle_root.as_bytes();
        writer.write_bytes(&merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        let next_consensus = self.next_consensus.as_bytes();
        writer.write_bytes(&next_consensus)?;
        Ok(())
    }

    fn validate_against_previous(
        &self,
        settings: &ProtocolSettings,
        prev_index: u32,
        prev_hash: &UInt256,
        prev_timestamp: u64,
    ) -> Result<(), &'static str> {
        if self.primary_index as i32 >= settings.validators_count {
            return Err("primary index exceeds validators count");
        }

        let Some(expected_index) = prev_index.checked_add(1) else {
            return Err("previous index overflow");
        };

        if expected_index != self.index {
            return Err("inconsistent block index");
        }

        if prev_hash != &self.prev_hash {
            return Err("previous hash mismatch");
        }

        if prev_timestamp >= self.timestamp {
            return Err("non-increasing timestamp");
        }

        Ok(())
    }

    fn verify_witness_against_hash(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        script_hash: &UInt160,
        witness: &Witness,
        gas_limit: i64,
    ) -> bool {
        debug!(
            target: "neo",
            %script_hash,
            gas_limit,
            "verifying witness against script hash"
        );
        if gas_limit < 0 {
            debug!(target: "neo", %script_hash, gas_limit, "gas limit below zero");
            return false;
        }

        let verification_gas = gas_limit.min(Helper::MAX_VERIFICATION_GAS);

        if neo_vm::Script::new(witness.invocation_script.clone(), true).is_err() {
            debug!(
                target: "neo",
                %script_hash,
                "invocation script is invalid"
            );
            return false;
        }

        let container: Arc<dyn crate::IVerifiable> = Arc::new(self.clone());
        let snapshot_arc = Arc::new(snapshot.clone());

        let mut engine = match ApplicationEngine::new(
            TriggerType::Verification,
            Some(container),
            snapshot_arc,
            None,
            settings.clone(),
            verification_gas,
            None,
        ) {
            Ok(engine) => engine,
            Err(_) => {
                debug!(
                    target: "neo",
                    %script_hash,
                    "failed to create application engine"
                );
                return false;
            }
        };

        let verification_script = witness.verification_script.clone();

        if verification_script.is_empty() {
            let contract =
                match ContractManagement::get_contract_from_snapshot(snapshot, script_hash) {
                    Ok(Some(contract)) => contract,
                    _ => {
                        debug!(
                            target: "neo",
                            %script_hash,
                            "contract not found for verification"
                        );
                        return false;
                    }
                };

            let mut abi = contract.manifest.abi.clone();
            let method = match abi.get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            ) {
                Some(descriptor) => descriptor.clone(),
                None => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        "verify method not found in contract ABI"
                    );
                    return false;
                }
            };

            if method.return_type != ContractParameterType::Boolean {
                debug!(
                    target: "neo",
                    %script_hash,
                    return_type = ?method.return_type,
                    "verify method return type is not boolean"
                );
                return false;
            }

            if engine
                .load_contract_method(contract, method, CallFlags::READ_ONLY)
                .is_err()
            {
                debug!(
                    target: "neo",
                    %script_hash,
                    "failed to load contract verification method"
                );
                return false;
            }
        } else {
            let witness_script_hash = witness.script_hash();
            debug!(
                target: "neo",
                %witness_script_hash,
                %script_hash,
                "comparing witness script hash with expected script hash"
            );
            if witness_script_hash != *script_hash {
                debug!(
                    target: "neo",
                    %witness_script_hash,
                    %script_hash,
                    "witness script hash mismatch"
                );
                return false;
            }

            if neo_vm::Script::new(verification_script.clone(), true).is_err() {
                debug!(
                    target: "neo",
                    %script_hash,
                    "verification script is invalid"
                );
                return false;
            }

            if engine
                .load_script_with_state(verification_script, -1, 0, |state| {
                    state.call_flags = CallFlags::READ_ONLY;
                    state.script_hash = Some(*script_hash);
                })
                .is_err()
            {
                debug!(
                    target: "neo",
                    %script_hash,
                    "failed to load verification script with state"
                );
                return false;
            }
        }

        if engine
            .load_script_with_state(witness.invocation_script.clone(), -1, 0, |state| {
                state.call_flags = CallFlags::NONE;
            })
            .is_err()
        {
            debug!(
                target: "neo",
                %script_hash,
                "failed to load invocation script with state"
            );
            return false;
        }

        if engine.execute().is_err() {
            debug!(target: "neo", %script_hash, "engine execution failed");
            return false;
        }

        let mut result_item = if engine.result_stack().len() == 1 {
            engine.result_stack().peek(0).ok()
        } else {
            None
        };

        if result_item.is_none() {
            if let Some(stack) = engine.current_evaluation_stack() {
                if stack.len() == 1 {
                    result_item = stack.peek(0).ok();
                }
            }
        }

        match result_item {
            Some(item) => match item.get_boolean() {
                Ok(result) => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        result,
                        "witness verification result from stack"
                    );
                    result
                }
                Err(err) => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        ?err,
                        "failed to read boolean result from stack"
                    );
                    false
                }
            },
            None => {
                debug!(
                    target: "neo",
                    %script_hash,
                    "result stack item missing"
                );
                false
            }
        }
    }
}

impl Header {
    /// Verifies the header using the provided store cache.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        let ledger = LedgerContract::new();
        let prev_trimmed = match ledger.get_trimmed_block(store_cache, &self.prev_hash) {
            Ok(Some(block)) => block,
            Ok(None) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    prev_hash = %self.prev_hash,
                    "verify: get_trimmed_block returned None for prev_hash"
                );
                return false;
            }
            Err(err) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    prev_hash = %self.prev_hash,
                    error = %err,
                    "verify: get_trimmed_block failed"
                );
                return false;
            }
        };

        let prev_index = prev_trimmed.index();
        let prev_hash = prev_trimmed.hash();
        let prev_header = prev_trimmed.header.clone();
        let prev_timestamp = prev_header.timestamp;
        let script_hash = prev_header.next_consensus;

        if let Err(reason) =
            self.validate_against_previous(settings, prev_index, &prev_hash, prev_timestamp)
        {
            debug!(
                target: "neo",
                index = self.index,
                %prev_hash,
                prev_index,
                %reason,
                "header failed validation against previous block"
            );
            return false;
        }

        let snapshot = store_cache.data_cache();
        let verified = self.verify_witness_against_hash(
            settings,
            snapshot,
            &script_hash,
            &self.witness,
            HEADER_VERIFY_GAS,
        );

        if !verified {
            debug!(
                target: "neo",
                index = self.index,
                %script_hash,
                prev_index,
                %prev_hash,
                "header witness verification failed against previous block"
            );
        }

        verified
    }

    /// Verifies the header using persisted state and cached headers.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        if let Some(mut prev_header) = header_cache.last() {
            let prev_hash = prev_header.hash();
            let prev_index = prev_header.index();
            let prev_timestamp = prev_header.timestamp();
            let script_hash = *prev_header.next_consensus();

            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                "verifying header index {} against previous index {}",
                self.index,
                prev_index
            );
            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                "attempting validate_against_previous"
            );
            if let Err(reason) =
                self.validate_against_previous(settings, prev_index, &prev_hash, prev_timestamp)
            {
                debug!(
                    target: "neo",
                    index = self.index,
                    %prev_hash,
                    prev_index,
                    %reason,
                    "header failed validation against cached previous"
                );
                return false;
            }

            let snapshot = store_cache.data_cache();
            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                %script_hash,
                "verifying witness against script_hash: {}",
                script_hash
            );
            let verified = self.verify_witness_against_hash(
                settings,
                snapshot,
                &script_hash,
                &self.witness,
                HEADER_VERIFY_GAS,
            );

            if !verified {
                debug!(
                    target: "neo",
                    index = self.index,
                    %script_hash,
                    prev_index,
                    %prev_hash,
                    failed_check = "verify_witness_against_hash",
                    "header witness verification failed against cached previous"
                );
            }

            return verified;
        }

        self.verify(settings, store_cache)
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        4 + 32
            + 32
            + 8
            + 8
            + 4
            + 1
            + 20
            + crate::neo_io::serializable::helper::get_var_size(1)
            + self.witness.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1 for header)
        writer.write_var_uint(1)?;
        writer.write_serializable(&self.witness)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u32()?;
        if version > 0 {
            return Err(IoError::invalid_data("unsupported header version"));
        }
        let prev_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let merkle_root = <UInt256 as Serializable>::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_u8()?;
        let next_consensus = <UInt160 as Serializable>::deserialize(reader)?;

        // Read witness count (should be 1)
        let witness_count = reader.read_var_uint()?;
        if witness_count != 1 {
            return Err(IoError::invalid_data("Invalid witness count for header"));
        }

        let witness = <Witness as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witness,
            _hash: None,
        })
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(Header);

impl crate::IVerifiable for Header {
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        if self.prev_hash == UInt256::default() {
            return vec![self.witness.script_hash()];
        }

        let ledger = LedgerContract::new();
        let prev = match ledger.get_trimmed_block(snapshot, &self.prev_hash) {
            Ok(Some(prev)) => prev,
            Ok(None) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash,
                    "previous header not found when verifying header"
                );
                return Vec::new();
            }
            Err(err) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash,
                    error = %err,
                    "failed to fetch previous header when verifying header"
                );
                return Vec::new();
            }
        };

        vec![prev.header.next_consensus]
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }

    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> CoreResult<UInt256> {
        let mut clone = self.clone();
        Ok(Header::hash(&mut clone))
    }

    fn get_hash_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(err) = self.serialize_unsigned(&mut writer) {
            tracing::error!("Failed to serialize header unsigned data: {err}");
            return Vec::new();
        }
        writer.into_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::block_header::BlockHeader as LedgerBlockHeader;
    use crate::ledger::HeaderCache;
    use crate::persistence::i_store::IStore;
    use crate::persistence::providers::memory_store::MemoryStore;
    use crate::persistence::StoreCache;
    use crate::smart_contract::native::trimmed_block::TrimmedBlock;
    use crate::smart_contract::storage_key::StorageKey;
    use crate::smart_contract::StorageItem;
    use crate::Witness as LedgerWitness;
    use neo_vm::op_code::OpCode;
    use std::sync::Arc;

    const LEDGER_CONTRACT_ID: i32 = -4;

    fn sample_witness() -> Witness {
        Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1 as u8])
    }

    fn sample_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default_settings();
        settings.validators_count = 7;
        settings
    }

    fn insert_trimmed_block(store_cache: &mut StoreCache, header: &Header, block_hash: UInt256) {
        let ledger_witness = LedgerWitness::new_with_scripts(
            header.witness.invocation_script.clone(),
            header.witness.verification_script.clone(),
        );

        let ledger_header = LedgerBlockHeader::new(
            header.version(),
            *header.prev_hash(),
            *header.merkle_root(),
            header.timestamp(),
            header.nonce(),
            header.index(),
            header.primary_index(),
            *header.next_consensus(),
            vec![ledger_witness],
        );

        let trimmed = TrimmedBlock::create(ledger_header, Vec::new());
        let mut writer = BinaryWriter::new();
        Serializable::serialize(&trimmed, &mut writer).expect("serialize trimmed block");
        let payload = writer.into_bytes();

        let mut key = Vec::with_capacity(1 + block_hash.to_bytes().len());
        key.push(5); // PREFIX_BLOCK
        key.extend_from_slice(&block_hash.to_bytes());

        store_cache.add(
            StorageKey::new(LEDGER_CONTRACT_ID, key),
            StorageItem::from_bytes(payload),
        );
    }

    #[test]
    fn verify_with_cache_succeeds_for_sequential_header() {
        let mut prev_header = Header::new();
        prev_header.set_version(0);
        prev_header.set_index(0);
        prev_header.set_timestamp(1_000);
        prev_header.set_primary_index(0);

        let deterministic_witness =
            Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT as u8, OpCode::RET as u8]);
        let consensus = deterministic_witness.script_hash();
        prev_header.witness = deterministic_witness.clone();
        prev_header.set_next_consensus(consensus);

        let mut prev_clone = prev_header.clone();
        let prev_hash = prev_clone.hash();

        let cache = HeaderCache::new();
        cache.add(prev_header.clone());

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(prev_hash);
        header.set_index(1);
        header.set_timestamp(2_000);
        header.set_primary_index(0);
        header.witness = deterministic_witness;

        let settings = sample_settings();
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(store, false);

        assert!(header.verify_with_cache(&settings, &store_cache, &cache));
    }

    #[test]
    fn verify_with_cache_rejects_when_timestamp_not_increasing() {
        let mut prev_header = Header::new();
        prev_header.set_version(0);
        prev_header.set_index(10);
        prev_header.set_timestamp(5_000);
        prev_header.set_primary_index(0);

        let prev_witness = sample_witness();
        let consensus = prev_witness.script_hash();
        prev_header.witness = prev_witness;
        prev_header.set_next_consensus(consensus);

        let mut prev_clone = prev_header.clone();
        let prev_hash = prev_clone.hash();

        let cache = HeaderCache::new();
        cache.add(prev_header.clone());

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(prev_hash);
        header.set_index(11);
        header.set_timestamp(5_000); // not strictly greater
        header.set_primary_index(0);
        header.witness = sample_witness();

        let settings = sample_settings();
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(store, true);

        assert!(!header.verify_with_cache(&settings, &store_cache, &cache));
    }

    #[test]
    fn verify_uses_persisted_state_when_cache_empty() {
        let mut prev_header = Header::new();
        prev_header.set_version(0);
        prev_header.set_index(20);
        prev_header.set_timestamp(7_500);
        prev_header.set_primary_index(0);

        let prev_witness = sample_witness();
        let consensus = prev_witness.script_hash();
        prev_header.witness = prev_witness;
        prev_header.set_next_consensus(consensus);

        let mut prev_clone = prev_header.clone();
        let prev_hash = prev_clone.hash();

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(prev_hash);
        header.set_index(21);
        header.set_timestamp(8_000);
        header.set_primary_index(0);
        header.witness = sample_witness();

        let settings = sample_settings();
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut store_cache = StoreCache::new_from_store(store, false);
        insert_trimmed_block(&mut store_cache, &prev_header, prev_hash);

        let ledger = LedgerContract::new();
        let prev_trimmed = ledger
            .get_trimmed_block(&store_cache, &prev_hash)
            .expect("trimmed block lookup")
            .expect("trimmed block should exist");

        let validation = header.validate_against_previous(
            &settings,
            prev_trimmed.index(),
            &prev_trimmed.hash(),
            prev_trimmed.header.timestamp,
        );
        assert!(
            validation.is_ok(),
            "validation failed: {:?}",
            validation.err()
        );

        let verified = header.verify_witness_against_hash(
            &settings,
            store_cache.data_cache(),
            &prev_trimmed.header.next_consensus,
            &header.witness,
            HEADER_VERIFY_GAS,
        );
        assert!(verified);

        assert!(header.verify(&settings, &store_cache));
    }
}
