//! LedgerContract native contract.
//!
//! Concrete (non-stub) implementation of the LedgerContract's storage
//! query surface. Mirrors the canonical C# `LedgerContract` storage
//! layout so plugins, services, and the application engine can read
//! transaction state, block-hash-by-index, and the current block
//! pointer that other components (blockchain, consensus) write into
//! the snapshot.
//!
//! The full read/write surface (block storage, block-hash index, the
//! various persistent transaction records and conflict stubs) is
//! handled by the `neo-blockchain` reth-style service; this crate only
//! provides the read-only query surface used by oracle service, RPC,
//! and the application engine.

use crate::hashes::LEDGER_CONTRACT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_payloads::Transaction;
use neo_primitives::{CallFlags, ContractParameterType, UInt160, UInt256};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;
use std::any::Any;
use std::sync::LazyLock;

/// Storage prefix for the per-block-index → block-hash index.
const PREFIX_BLOCK_HASH: u8 = 9;
/// Storage prefix for the trimmed-block payload.
const PREFIX_BLOCK: u8 = 5;
/// Storage prefix for the per-transaction state record.
const PREFIX_TRANSACTION: u8 = 11;
/// Storage prefix for the current-block (hash, index) pointer.
const PREFIX_CURRENT_BLOCK: u8 = 12;

/// Record-kind tag identifying a full persisted transaction record.
const RECORD_KIND_TRANSACTION: u8 = 0x01;
/// Record-kind tag identifying a conflict-stub record.
const RECORD_KIND_CONFLICT_STUB: u8 = 0x02;

/// Maximum supported transaction byte length (matches C#
/// `Transaction.MaxTransactionSize`).
const MAX_TRANSACTION_SIZE: usize = 102_400;

/// Lazily-initialised script-hash handle for the LedgerContract.
pub static LEDGER_HASH: LazyLock<UInt160> = LazyLock::new(|| *LEDGER_CONTRACT_HASH);

/// Static accessor for the LedgerContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct LedgerContract;

impl LedgerContract {
    /// Stable native contract id (matches C# `LedgerContract.Id`).
    pub const ID: i32 = -4;

    /// Constructs a new `LedgerContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the LedgerContract.
    pub fn hash(&self) -> UInt160 {
        *LEDGER_HASH
    }

    /// Returns the script hash of the LedgerContract (static).
    pub fn script_hash() -> UInt160 {
        *LEDGER_HASH
    }

    /// Returns the current block index (height) of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. Returns `0` when the pointer is
    /// missing (e.g. at genesis).
    pub fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        let key = current_block_storage_key(Self::ID);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let (_, index) = deserialize_hash_index_state(&bytes)?;
                Ok(index)
            }
            None => Ok(0),
        }
    }

    /// Returns the current block hash of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. Returns the zero hash when the pointer
    /// is missing.
    pub fn current_hash(&self, snapshot: &DataCache) -> CoreResult<UInt256> {
        let key = current_block_storage_key(Self::ID);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let (hash, _) = deserialize_hash_index_state(&bytes)?;
                Ok(hash)
            }
            None => Ok(UInt256::default()),
        }
    }

    /// Returns the per-transaction state for the given transaction
    /// hash, or `None` if the transaction is not in the ledger.
    ///
    /// The on-disk format (prefix `11` + 32-byte hash) is:
    /// ```text
    /// u8  record_kind (0x01 = full transaction, 0x02 = conflict stub)
    /// u32 block_index
    /// u8  vm_state             (only when record_kind == 0x01)
    /// var transaction_bytes    (only when record_kind == 0x01)
    /// ```
    pub fn get_transaction_state(
        &self,
        snapshot: &DataCache,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<neo_block::TransactionState>> {
        let key = transaction_storage_key(Self::ID, tx_hash);
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };

        let bytes = item.value_bytes().into_owned();
        let mut reader = MemoryReader::new(&bytes);
        let kind = reader
            .read_u8()
            .map_err(|e| CoreError::invalid_data(format!("invalid record kind: {e}")))?;

        match kind {
            RECORD_KIND_TRANSACTION => {
                let block_index = reader
                    .read_u32()
                    .map_err(|e| CoreError::invalid_data(format!("invalid block index: {e}")))?;
                let vm_state_byte = reader
                    .read_u8()
                    .map_err(|e| CoreError::invalid_data(format!("invalid vm state: {e}")))?;
                let tx_bytes = reader
                    .read_var_bytes(MAX_TRANSACTION_SIZE)
                    .map_err(|e| {
                        CoreError::invalid_data(format!("invalid transaction bytes: {e}"))
                    })?;
                let mut tx_reader = MemoryReader::new(&tx_bytes);
                let tx = Transaction::deserialize(&mut tx_reader)
                    .map_err(|e| CoreError::serialization(e.to_string()))?;

                Ok(Some(neo_block::TransactionState::new(
                    block_index,
                    Some(tx),
                    VMState::from_byte(vm_state_byte),
                )))
            }
            RECORD_KIND_CONFLICT_STUB => {
                let block_index = reader
                    .read_u32()
                    .map_err(|e| CoreError::invalid_data(format!("invalid conflict block index: {e}")))?;
                Ok(Some(neo_block::TransactionState::new(
                    block_index,
                    None,
                    VMState::NONE,
                )))
            }
            _ => Err(CoreError::invalid_data(
                "unknown transaction state record kind",
            )),
        }
    }

    /// Returns whether the given transaction is present in the ledger
    /// (either as a full record or as a conflict stub).
    pub fn contains_transaction(
        &self,
        snapshot: &DataCache,
        tx_hash: &UInt256,
    ) -> CoreResult<bool> {
        Ok(self.get_transaction_state(snapshot, tx_hash)?.is_some())
    }

    /// Returns the block hash for the given block index, or `None` if
    /// the block has not been persisted yet.
    pub fn get_block_hash(
        &self,
        snapshot: &DataCache,
        index: u32,
    ) -> CoreResult<Option<UInt256>> {
        let key = block_hash_storage_key(Self::ID, index);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let hash = UInt256::from_bytes(&bytes).map_err(|e| {
                    CoreError::invalid_data(format!("invalid block hash bytes: {e}"))
                })?;
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }
}

// ============================================================================
// Storage-key helpers
// ============================================================================

#[inline]
fn current_block_storage_key(contract_id: i32) -> StorageKey {
    StorageKey::new(contract_id, vec![PREFIX_CURRENT_BLOCK])
}

#[inline]
fn block_hash_storage_key(contract_id: i32, index: u32) -> StorageKey {
    let mut key = Vec::with_capacity(1 + std::mem::size_of::<u32>());
    key.push(PREFIX_BLOCK_HASH);
    key.extend_from_slice(&index.to_le_bytes());
    StorageKey::new(contract_id, key)
}

#[inline]
fn transaction_storage_key(contract_id: i32, hash: &UInt256) -> StorageKey {
    let mut key = Vec::with_capacity(1 + 32);
    key.push(PREFIX_TRANSACTION);
    key.extend_from_slice(&hash.to_bytes());
    StorageKey::new(contract_id, key)
}

// ============================================================================
// Wire-format helpers
// ============================================================================

/// Serialises a `(hash, index)` pair into the C#-compatible
/// `HashIndexState` wire format used for the current-block pointer.
pub fn serialize_hash_index_state(hash: &UInt256, index: u32) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_bytes(&hash.to_bytes())
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    writer
        .write_u32(index)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    Ok(writer.into_bytes())
}

fn deserialize_hash_index_state(bytes: &[u8]) -> CoreResult<(UInt256, u32)> {
    if bytes.len() < 36 {
        return Err(CoreError::invalid_data(
            "HashIndexState payload is shorter than expected",
        ));
    }
    let hash = UInt256::from_bytes(&bytes[..32])
        .map_err(|e| CoreError::invalid_data(format!("invalid HashIndexState hash: {e}")))?;
    let mut index_bytes = [0u8; 4];
    index_bytes.copy_from_slice(&bytes[32..36]);
    let index = u32::from_le_bytes(index_bytes);
    Ok((hash, index))
}

/// Serialises a persisted transaction state into the C#-compatible
/// wire format. Useful for tests and the persistence pipeline.
pub fn serialize_persisted_transaction_state(
    block_index: u32,
    vm_state: VMState,
    tx: &Transaction,
) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_u8(RECORD_KIND_TRANSACTION)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    writer
        .write_u32(block_index)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    writer
        .write_u8(vm_state.to_byte())
        .map_err(|e| CoreError::serialization(e.to_string()))?;

    let mut tx_writer = BinaryWriter::new();
    tx.serialize(&mut tx_writer)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    writer
        .write_var_bytes(&tx_writer.into_bytes())
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    Ok(writer.into_bytes())
}

/// Serialises a conflict-stub record into the C#-compatible wire
/// format. Useful for tests and the persistence pipeline.
pub fn serialize_conflict_stub(block_index: u32) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_u8(RECORD_KIND_CONFLICT_STUB)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    writer
        .write_u32(block_index)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    Ok(writer.into_bytes())
}

static LEDGER_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "currentHash".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash256,
        ),
        NativeMethod::new(
            "currentIndex".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
    ]
});

impl NativeContract for LedgerContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *LEDGER_CONTRACT_HASH
    }

    fn name(&self) -> &str {
        "LedgerContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &LEDGER_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Both wired methods are read-only queries over persisted ledger state,
        // served from the engine's snapshot (C# `RequiredCallFlags = ReadStates`).
        let snapshot = engine.snapshot_cache();
        match method {
            "currentIndex" => Ok(BigInt::from(self.current_index(&snapshot)?).to_signed_bytes_le()),
            "currentHash" => Ok(self.current_hash(&snapshot)?.to_bytes()),
            other => Err(CoreError::invalid_operation(format!(
                "LedgerContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = LedgerContract::new();
        assert_eq!(NativeContract::id(&c), -4);
        assert_eq!(NativeContract::name(&c), "LedgerContract");
        assert_eq!(NativeContract::hash(&c), *LEDGER_CONTRACT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["currentHash", "currentIndex"]);
        assert!(c
            .methods()
            .iter()
            .all(|m| m.safe && m.required_call_flags == CallFlags::READ_STATES.bits()));
    }

    #[test]
    fn current_index_and_hash_round_trip_through_storage() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        // Empty ledger: index 0, zero hash (C# returns these when the
        // current-block pointer is absent).
        assert_eq!(ledger.current_index(&cache).unwrap(), 0);
        assert_eq!(ledger.current_hash(&cache).unwrap(), UInt256::default());

        // Write a HashIndexState under the current-block key (prefix 12) and
        // read it back, exercising the exact on-disk format the engine uses.
        let hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
        let bytes = serialize_hash_index_state(&hash, 1234).unwrap();
        cache.add(
            current_block_storage_key(LedgerContract::ID),
            StorageItem::from_bytes(bytes),
        );
        assert_eq!(ledger.current_index(&cache).unwrap(), 1234);
        assert_eq!(ledger.current_hash(&cache).unwrap(), hash);
    }
}
