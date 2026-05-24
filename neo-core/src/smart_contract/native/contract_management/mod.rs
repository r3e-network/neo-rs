//! ContractManagement native contract - complete production implementation.
//!
//! This module provides the ContractManagement native contract which manages
//! all deployed smart contracts on the Neo blockchain.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::neo_io::{MemoryReader, Serializable};
use crate::neo_vm::StackItem;
use crate::persistence::{DataCache, StoreCache};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::contract_state::{ContractState, NefFile};
use crate::smart_contract::manifest::ContractManifest;
use crate::smart_contract::native::{NativeContract, NativeMethod, PolicyContract};
use crate::smart_contract::StorageKey;
use crate::UInt160;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Prefix for minimum deployment fee storage
const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// Prefix for next available contract ID storage
const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
/// Prefix for contract storage
const PREFIX_CONTRACT: u8 = 8;
/// Prefix for contract hash by ID storage
const PREFIX_CONTRACT_HASH: u8 = 12;
/// Prefix for contract count
const PREFIX_CONTRACT_COUNT: u8 = 16;

/// Default minimum deployment fee (10 GAS)
const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Contract storage state
#[derive(Debug, Clone, Default)]
pub(super) struct ContractStorage {
    /// All deployed contracts by hash
    pub(super) contracts: HashMap<UInt160, ContractState>,
    /// Contract hashes by ID
    pub(super) contract_ids: HashMap<i32, UInt160>,
    /// Next available contract ID
    pub(super) next_id: i32,
    /// Minimum deployment fee
    pub(super) minimum_deployment_fee: i64,
    /// Total number of contracts
    pub(super) contract_count: u32,
}

/// ContractManagement native contract
pub struct ContractManagement {
    pub(super) id: i32,
    pub(super) hash: UInt160,
    pub(super) methods: Vec<NativeMethod>,
    pub(super) storage: Arc<RwLock<ContractStorage>>,
}

impl ContractManagement {
    const ID: i32 = -1;

    pub fn contract_hash() -> UInt160 {
        UInt160::parse("0xfffdc93764dbaddd97c48f252a53ea4643faa3fd")
            .expect("Valid ContractManagement contract hash")
    }

    #[inline]
    fn storage_key(prefix: u8, suffix: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + suffix.len());
        key.push(prefix);
        key.extend_from_slice(suffix);
        key
    }

    /// Builds the storage key for a contract state entry (prefix + script hash).
    #[inline]
    pub fn contract_storage_key(hash: &UInt160) -> Vec<u8> {
        Self::storage_key(PREFIX_CONTRACT, hash.as_bytes().as_ref())
    }

    #[inline]
    fn contract_id_storage_key(id: i32) -> Vec<u8> {
        let bytes = id.to_be_bytes();
        Self::storage_key(PREFIX_CONTRACT_HASH, bytes.as_ref())
    }

    #[inline]
    fn contract_id_storage_key_legacy(id: i32) -> Vec<u8> {
        let bytes = id.to_le_bytes();
        Self::storage_key(PREFIX_CONTRACT_HASH, bytes.as_ref())
    }

    #[cfg(test)]
    #[inline]
    fn contract_count_key() -> Vec<u8> {
        vec![PREFIX_CONTRACT_COUNT]
    }

    #[inline]
    fn next_id_key() -> Vec<u8> {
        vec![PREFIX_NEXT_AVAILABLE_ID]
    }

    #[inline]
    fn minimum_deployment_fee_key() -> Vec<u8> {
        vec![PREFIX_MINIMUM_DEPLOYMENT_FEE]
    }

    #[inline]
    fn encode_storage_bigint(value: BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    #[inline]
    fn encode_storage_i32(value: i32) -> Vec<u8> {
        Self::encode_storage_bigint(BigInt::from(value))
    }

    #[inline]
    fn encode_storage_i64(value: i64) -> Vec<u8> {
        Self::encode_storage_bigint(BigInt::from(value))
    }

    #[inline]
    fn decode_storage_i32(bytes: &[u8]) -> Option<i32> {
        if bytes.is_empty() {
            return Some(0);
        }
        BigInt::from_signed_bytes_le(bytes).to_i32()
    }

    #[inline]
    fn decode_storage_i64(bytes: &[u8]) -> Option<i64> {
        if bytes.is_empty() {
            return Some(0);
        }
        BigInt::from_signed_bytes_le(bytes).to_i64()
    }

    #[inline]
    fn decode_storage_u32(bytes: &[u8]) -> Option<u32> {
        if bytes.is_empty() {
            return Some(0);
        }
        BigInt::from_signed_bytes_le(bytes).to_u32()
    }

    #[inline]
    fn stack_item_from_payload(data: &[u8]) -> StackItem {
        if data.is_empty() {
            return StackItem::null();
        }

        BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
            .unwrap_or_else(|_| StackItem::from_byte_string(data.to_vec()))
    }

    pub(super) fn serialize_contract_state(contract: &ContractState) -> Result<Vec<u8>> {
        let value: StackValue = contract.to_stack_value();
        BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
            .map_err(|e| Error::serialization(format!("Failed to serialize contract state: {e}")))
    }

    pub fn deserialize_contract_state(bytes: &[u8]) -> Result<ContractState> {
        if bytes.is_empty() {
            return Err(Error::deserialization(
                "Contract state payload is empty".to_string(),
            ));
        }

        if let Ok(value) = BinarySerializer::deserialize_stack_value(bytes) {
            let mut contract = ContractState::default();
            if contract.from_stack_value(value).is_ok() {
                return Ok(contract);
            }
        }

        let mut reader = MemoryReader::new(bytes);
        <ContractState as Serializable>::deserialize(&mut reader).map_err(|e| {
            Error::deserialization(format!("Failed to deserialize contract state: {e}"))
        })
    }

    fn invoke_deploy_hook(
        &self,
        engine: &mut ApplicationEngine,
        contract_hash: &UInt160,
        data: &[u8],
        is_update: bool,
    ) -> Result<()> {
        let args = vec![
            Self::stack_item_from_payload(data),
            StackItem::from_bool(is_update),
        ];
        engine.queue_contract_call_from_native(self.hash, *contract_hash, "_deploy", args);
        Ok(())
    }

    /// Creates a new ContractManagement instance
    pub fn new() -> Self {
        // ContractManagement contract hash: 0xfffdc93764dbaddd97c48f252a53ea4643faa3fd
        let hash = Self::contract_hash();

        let storage = ContractStorage {
            contracts: HashMap::new(),
            contract_ids: HashMap::new(),
            next_id: 1,
            minimum_deployment_fee: DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            contract_count: 0,
        };

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
            storage: Arc::new(RwLock::new(storage)),
        }
    }
}

// Include implementation files
mod deploy;
mod destroy;
mod metadata;
mod native_impl;
mod query;
#[cfg(test)]
mod tests;
mod update;
mod validation;
