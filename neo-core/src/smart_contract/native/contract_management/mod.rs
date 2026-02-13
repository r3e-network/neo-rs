//! ContractManagement native contract - complete production implementation.
//!
//! This module provides the ContractManagement native contract which manages
//! all deployed smart contracts on the Neo blockchain.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::neo_io::{MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::contract_state::{ContractState, NefFile};
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::ContractManifest;
use crate::smart_contract::native::{NativeContract, NativeMethod, PolicyContract};
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use neo_vm::{ExecutionEngineLimits, StackItem};
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
    fn stack_item_from_payload(data: &[u8]) -> StackItem {
        if data.is_empty() {
            return StackItem::null();
        }

        BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
            .unwrap_or_else(|_| StackItem::from_byte_string(data.to_vec()))
    }

    pub(super) fn serialize_contract_state(contract: &ContractState) -> Result<Vec<u8>> {
        BinarySerializer::serialize(
            &contract.to_stack_item()?,
            &ExecutionEngineLimits::default(),
        )
        .map_err(|e| Error::serialization(format!("Failed to serialize contract state: {e}")))
    }

    pub fn deserialize_contract_state(bytes: &[u8]) -> Result<ContractState> {
        if bytes.is_empty() {
            return Err(Error::deserialization(
                "Contract state payload is empty".to_string(),
            ));
        }

        if let Ok(item) =
            BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
        {
            if let Ok(contract) = Self::contract_state_from_stack_item(item) {
                return Ok(contract);
            }
        }

        let mut reader = MemoryReader::new(bytes);
        <ContractState as Serializable>::deserialize(&mut reader).map_err(|e| {
            Error::deserialization(format!("Failed to deserialize contract state: {e}"))
        })
    }

    fn contract_state_from_stack_item(item: StackItem) -> Result<ContractState> {
        let items = match item {
            StackItem::Array(array) => array.items(),
            StackItem::Struct(struct_item) => struct_item.items(),
            _ => {
                return Err(Error::deserialization(
                    "ContractState stack item must be array or struct".to_string(),
                ))
            }
        };

        if items.len() < 5 {
            return Err(Error::deserialization(
                "ContractState stack item must contain five elements".to_string(),
            ));
        }

        let id = items[0]
            .as_int()
            .map_err(|e| Error::deserialization(format!("ContractState id: {e}")))?;
        let id = id
            .to_i32()
            .ok_or_else(|| Error::deserialization("ContractState id out of range".to_string()))?;

        let update_counter = items[1]
            .as_int()
            .map_err(|e| Error::deserialization(format!("ContractState update counter: {e}")))?;
        let update_counter = update_counter.to_u16().ok_or_else(|| {
            Error::deserialization("ContractState update counter out of range".to_string())
        })?;

        let hash_bytes = items[2]
            .as_bytes()
            .map_err(|e| Error::deserialization(format!("ContractState hash: {e}")))?;
        let hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| Error::deserialization(format!("ContractState hash: {e}")))?;

        let nef_bytes = items[3]
            .as_bytes()
            .map_err(|e| Error::deserialization(format!("ContractState NEF: {e}")))?;
        let nef = NefFile::parse(&nef_bytes)
            .map_err(|e| Error::deserialization(format!("ContractState NEF parse: {e}")))?;

        let mut manifest = ContractManifest::new(String::new());
        manifest
            .from_stack_item(items[4].clone())
            .map_err(|e| Error::deserialization(format!("ContractState manifest: {e}")))?;

        if hash.is_zero() || nef.script.is_empty() || manifest.name.is_empty() {
            return Err(Error::deserialization(
                "ContractState stack item is invalid".to_string(),
            ));
        }

        Ok(ContractState {
            id,
            update_counter,
            hash,
            nef,
            manifest,
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

        let methods = vec![
            NativeMethod::new(
                "getContract".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            ),
            NativeMethod::new(
                "deploy".to_string(),
                0,
                false,
                0x0B,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Array,
            ),
            NativeMethod::new(
                "deploy".to_string(),
                0,
                false,
                0x0B,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Array,
            ),
            NativeMethod::new(
                "update".to_string(),
                0,
                false,
                0x0B,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Void,
            ),
            NativeMethod::new(
                "update".to_string(),
                0,
                false,
                0x0B,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            ),
            NativeMethod::new(
                "destroy".to_string(),
                1 << 15,
                false,
                0x0B,
                Vec::new(),
                ContractParameterType::Void,
            ),
            NativeMethod::new(
                "getMinimumDeploymentFee".to_string(),
                1 << 15,
                true,
                0x01,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::new(
                "setMinimumDeploymentFee".to_string(),
                1 << 15,
                false,
                0x03,
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            ),
            NativeMethod::new(
                "hasMethod".to_string(),
                1 << 15,
                true,
                0x01,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            ),
            NativeMethod::new(
                "getContractById".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Integer],
                ContractParameterType::Array,
            ),
            NativeMethod::new(
                "isContract".to_string(),
                1 << 14,
                true,
                0x01,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_active_in(crate::hardfork::Hardfork::HfEchidna),
            NativeMethod::new(
                "getContractHashes".to_string(),
                1 << 15,
                true,
                0x01,
                Vec::new(),
                ContractParameterType::InteropInterface,
            ),
        ];
        let methods = methods
            .into_iter()
            .map(|method| match method.name.as_str() {
                "getContract" => method.with_parameter_names(vec!["hash".to_string()]),
                "deploy" if method.parameters.len() == 2 => {
                    method.with_parameter_names(vec!["nefFile".to_string(), "manifest".to_string()])
                }
                "deploy" => method.with_parameter_names(vec![
                    "nefFile".to_string(),
                    "manifest".to_string(),
                    "data".to_string(),
                ]),
                "update" if method.parameters.len() == 2 => {
                    method.with_parameter_names(vec!["nefFile".to_string(), "manifest".to_string()])
                }
                "update" => method.with_parameter_names(vec![
                    "nefFile".to_string(),
                    "manifest".to_string(),
                    "data".to_string(),
                ]),
                "setMinimumDeploymentFee" => method.with_parameter_names(vec!["value".to_string()]),
                "hasMethod" => method.with_parameter_names(vec![
                    "hash".to_string(),
                    "method".to_string(),
                    "pcount".to_string(),
                ]),
                "getContractById" => method.with_parameter_names(vec!["id".to_string()]),
                "isContract" => method.with_parameter_names(vec!["hash".to_string()]),
                _ => method,
            })
            .collect();

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
            methods,
            storage: Arc::new(RwLock::new(storage)),
        }
    }
}

// Include implementation files
mod deploy;
mod destroy;
mod native_impl;
mod query;
#[cfg(test)]
mod tests;
mod update;
mod validation;
