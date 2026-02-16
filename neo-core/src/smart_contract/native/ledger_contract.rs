//! Native ledger contract: manages blocks and transactions on-chain.

use self::keys::{
    block_hash_storage_key, block_storage_key, current_block_storage_key,
    transaction_conflict_storage_key, transaction_storage_key,
};
use self::state::{
    TransactionStateRecord, deserialize_hash_index_state, deserialize_transaction_record,
    deserialize_trimmed_block, serialize_hash_index_state, serialize_transaction_record,
    serialize_trimmed_block,
};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::ledger::Block;
use crate::persistence::{DataCache, i_read_only_store::IReadOnlyStoreGeneric};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::IInteroperable;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::{
    NativeMethod, policy_contract::PolicyContract, trimmed_block::TrimmedBlock,
};
use crate::smart_contract::{StorageItem, StorageKey};
use crate::{UInt160, UInt256};
use neo_vm::StackItem;
use neo_vm::vm_state::VMState;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Prefix for block-hash-by-index storage
const PREFIX_BLOCK_HASH: u8 = 9;
/// Prefix for block storage (trimmed block payloads)
const PREFIX_BLOCK: u8 = 5;
/// Prefix for transaction state storage
const PREFIX_TRANSACTION: u8 = 11;
/// Prefix for current block pointer storage
const PREFIX_CURRENT_BLOCK: u8 = 12;

mod helpers;
pub(crate) mod keys;
mod native_impl;
mod state;
mod storage;
#[cfg(test)]
mod tests;
pub use state::{LedgerTransactionStates, PersistedTransactionState};

/// LedgerContract native contract
pub struct LedgerContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl LedgerContract {
    pub const ID: i32 = -4;

    /// Creates a new LedgerContract instance
    pub fn new() -> Self {
        // LedgerContract hash: 0xda65b600f7124ce6c79950c1772a36403104f2be
        let hash = UInt160::parse("0xda65b600f7124ce6c79950c1772a36403104f2be")
            .expect("Valid LedgerContract hash");

        let methods = vec![
            NativeMethod::new(
                "currentHash".to_string(),
                1 << 15,
                true,
                0x01,
                Vec::new(),
                ContractParameterType::Hash256,
            ),
            NativeMethod::new(
                "currentIndex".to_string(),
                1 << 15,
                true,
                0x01,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::new(
                "getBlock".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["indexOrHash".to_string()]),
            NativeMethod::new(
                "getTransaction".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionFromBlock".to_string(),
                1 << 16,
                true,
                0x01,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["blockIndexOrHash".to_string(), "txIndex".to_string()]),
            NativeMethod::new(
                "getTransactionHeight".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionSigners".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionVMState".to_string(),
                1 << 15,
                true,
                0x01,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["hash".to_string()]),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Gets the current block hash from the persisted state.
    pub fn current_hash<S>(&self, snapshot: &S) -> Result<UInt256>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = current_block_storage_key(self.id);
        if let Some(item) = snapshot.try_get(&key) {
            let state = deserialize_hash_index_state(&item.get_value())?;
            return Ok(state.hash);
        }
        Ok(UInt256::default())
    }

    /// Gets the current block index (height) from the persisted state.
    pub fn current_index<S>(&self, snapshot: &S) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = current_block_storage_key(self.id);
        if let Some(item) = snapshot.try_get(&key) {
            let state = deserialize_hash_index_state(&item.get_value())?;
            return Ok(state.index);
        }
        Ok(0)
    }

    /// Retrieves a block by hash or index.
    pub fn get_block<S>(&self, snapshot: &S, hash_or_index: HashOrIndex) -> Result<Option<Block>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        match hash_or_index {
            HashOrIndex::Hash(hash) => self.try_read_block(snapshot, &hash),
            HashOrIndex::Index(index) => {
                if let Some(hash) = self.load_block_hash(snapshot, index)? {
                    self.try_read_block(snapshot, &hash)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Checks whether a block exists in storage.
    pub fn contains_block<S>(&self, snapshot: &S, hash: &UInt256) -> bool
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_storage_key(self.id, hash);
        snapshot.try_get(&key).is_some()
    }

    /// Checks whether a transaction exists in storage.
    pub fn contains_transaction<S>(&self, snapshot: &S, hash: &UInt256) -> Result<bool>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        Ok(self.try_read_transaction_state(snapshot, hash)?.is_some())
    }

    pub fn contains_conflict_hash<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> Result<bool>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if signers.is_empty() {
            return Ok(false);
        }

        let current_index = self.current_index(snapshot)?;
        let key = transaction_storage_key(self.id, hash);

        let Some(TransactionStateRecord::ConflictStub {
            block_index: stub_index,
        }) = self.read_transaction_record(snapshot, &key)?
        else {
            return Ok(false);
        };

        if !Self::is_traceable_block(current_index, stub_index, max_traceable_blocks) {
            return Ok(false);
        }

        for signer in signers {
            let signer_key = transaction_conflict_storage_key(self.id, hash, signer);
            if let Some(TransactionStateRecord::ConflictStub { block_index }) =
                self.read_transaction_record(snapshot, &signer_key)?
            {
                if Self::is_traceable_block(current_index, block_index, max_traceable_blocks) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    pub fn max_traceable_blocks_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let current_index = self.current_index(snapshot)?;
        let mut value = if settings.is_hardfork_enabled(Hardfork::HfEchidna, current_index) {
            max_traceable_blocks_from_snapshot(snapshot, settings.max_traceable_blocks)
        } else {
            settings.max_traceable_blocks
        };

        if value == 0 {
            value = settings.max_traceable_blocks;
        }

        value = value.min(PolicyContract::MAX_MAX_TRACEABLE_BLOCKS);
        Ok(value.max(1))
    }
}

/// Hash or index parameter for block queries
pub enum HashOrIndex {
    Hash(UInt256),
    Index(u32),
}

fn max_traceable_blocks_from_snapshot<S>(snapshot: &S, default: u32) -> u32
where
    S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
{
    PolicyContract::get_max_traceable_blocks_snapshot(snapshot)
        .filter(|&value| value > 0)
        .unwrap_or(default)
}
