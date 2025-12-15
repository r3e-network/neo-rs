//! ContractManagement native contract - complete production implementation.
//!
//! This module provides the ContractManagement native contract which manages
//! all deployed smart contracts on the Neo blockchain.

use crate::cryptography::Crypto;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::contract_state::{ContractState, NefFile};
use crate::smart_contract::manifest::{ContractManifest, ContractPermissionDescriptor};
use crate::smart_contract::native::{NativeContract, NativeMethod, PolicyContract};
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use neo_vm::{ExecutionEngineLimits, StackItem};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::convert::TryInto;
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

    const HASH_BYTES: [u8; 20] = [
        0xff, 0xfd, 0xc9, 0x37, 0x64, 0xdb, 0xad, 0xdd, 0x97, 0xc4, 0x8f, 0x25, 0x2a, 0x53, 0xea,
        0x46, 0x43, 0xfa, 0xa3, 0xfd,
    ];

    pub fn contract_hash() -> UInt160 {
        UInt160::from(Self::HASH_BYTES)
    }

    #[inline]
    fn storage_key(prefix: u8, suffix: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + suffix.len());
        key.push(prefix);
        key.extend_from_slice(suffix);
        key
    }

    #[inline]
    fn contract_storage_key(hash: &UInt160) -> Vec<u8> {
        Self::storage_key(PREFIX_CONTRACT, hash.as_bytes().as_ref())
    }

    #[inline]
    fn contract_id_storage_key(id: i32) -> Vec<u8> {
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
                ContractParameterType::ByteArray,
            ),
            NativeMethod::new(
                "deploy".to_string(),
                0,
                false,
                0x0F,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Any,
                ],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::new(
                "update".to_string(),
                0,
                false,
                0x0F,
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
                0x0F,
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
                0x0F,
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
                ContractParameterType::ByteArray,
            ),
            NativeMethod::new(
                "getContractHashes".to_string(),
                1 << 15,
                true,
                0x01,
                Vec::new(),
                ContractParameterType::ByteArray,
            ),
        ];

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
mod validation;
mod deploy;
mod update;
mod destroy;
mod query;
mod native_impl;
