//! Ledger contract state types and serialization helpers.
//!
//! Migrated from `neo-core/src/smart_contract/native/ledger_contract/state.rs`.
//!
//! Import adaptation:
//! - `crate::UInt256` → `neo_primitives::UInt256`
//! - `crate::error::{CoreError, CoreResult}` → `neo_smart_contract::error::{CoreError, CoreResult}`
//! - `crate::neo_io::{BinaryWriter, MemoryReader, Serializable}` → `neo_io::{BinaryWriter, MemoryReader, Serializable}`
//! - `crate::network::p2p::payloads::transaction::{MAX_TRANSACTION_SIZE, Transaction}` → `neo_protocol::payloads::transaction::{MAX_TRANSACTION_SIZE, Transaction}`
//! - `crate::smart_contract::application_engine::execution_state::*` → `neo_smart_contract::application_engine::execution_state::*`
//! - `crate::smart_contract::native::{hash_index_state::HashIndexState, trimmed_block::TrimmedBlock}` → `crate::native::{hash_index_state::HashIndexState, trimmed_block::TrimmedBlock}`

use crate::native::{hash_index_state::HashIndexState, trimmed_block::TrimmedBlock};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_protocol::payloads::transaction::{MAX_TRANSACTION_SIZE, Transaction};
pub use neo_smart_contract::application_engine::execution_state::{
    LedgerTransactionStates, PersistedTransactionState,
};
use neo_smart_contract::error::{CoreError as Error, CoreResult as Result};
use neo_vm::VmState as VMState;

const RECORD_KIND_TRANSACTION: u8 = 0x01;
const RECORD_KIND_CONFLICT_STUB: u8 = 0x02;

#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub enum TransactionStateRecord {
    Full(PersistedTransactionState),
    ConflictStub { block_index: u32 },
}

pub fn serialize_trimmed_block(trimmed: &TrimmedBlock) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    <TrimmedBlock as Serializable>::serialize(trimmed, &mut writer)
        .map_err(|e| Error::serialization(e.to_string()))?;
    Ok(writer.to_bytes())
}

pub fn deserialize_trimmed_block(bytes: &[u8]) -> Result<TrimmedBlock> {
    let mut reader = MemoryReader::new(bytes);
    <TrimmedBlock as Serializable>::deserialize(&mut reader)
        .map_err(|e| Error::serialization(e.to_string()))
}

pub fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    <Transaction as Serializable>::serialize(tx, &mut writer)
        .map_err(|e| Error::serialization(e.to_string()))?;
    Ok(writer.to_bytes())
}

pub fn serialize_transaction_record(record: &TransactionStateRecord) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    match record {
        TransactionStateRecord::Full(state) => {
            writer
                .write_u8(RECORD_KIND_TRANSACTION)
                .map_err(|e| Error::serialization(e.to_string()))?;
            writer
                .write_u32(state.block_index())
                .map_err(|e| Error::serialization(e.to_string()))?;
            writer
                .write_u8(state.vm_state_raw())
                .map_err(|e| Error::serialization(e.to_string()))?;
            let tx_bytes = serialize_transaction(state.transaction())?;
            writer
                .write_var_bytes(&tx_bytes)
                .map_err(|e| Error::serialization(e.to_string()))?;
        }
        TransactionStateRecord::ConflictStub { block_index } => {
            writer
                .write_u8(RECORD_KIND_CONFLICT_STUB)
                .map_err(|e| Error::serialization(e.to_string()))?;
            writer
                .write_u32(*block_index)
                .map_err(|e| Error::serialization(e.to_string()))?;
        }
    }

    Ok(writer.to_bytes())
}

pub fn deserialize_transaction_record(bytes: &[u8]) -> Result<TransactionStateRecord> {
    if bytes.is_empty() {
        return Err(Error::invalid_data(
            "transaction state record payload is empty",
        ));
    }

    let mut reader = MemoryReader::new(bytes);
    let kind = reader
        .read_u8()
        .map_err(|e| Error::invalid_data(format!("invalid record kind: {e}")))?;

    match kind {
        RECORD_KIND_TRANSACTION => {
            let block_index = reader
                .read_u32()
                .map_err(|e| Error::invalid_data(format!("invalid block index: {e}")))?;
            let vm_state = reader
                .read_u8()
                .map_err(|e| Error::invalid_data(format!("invalid vm state: {e}")))?;
            let tx_bytes = reader
                .read_var_bytes(MAX_TRANSACTION_SIZE)
                .map_err(|e| Error::invalid_data(format!("invalid transaction bytes: {e}")))?;
            let mut tx_reader = MemoryReader::new(&tx_bytes);
            let tx = <Transaction as Serializable>::deserialize(&mut tx_reader)
                .map_err(|e| Error::serialization(e.to_string()))?;

            let mut state = PersistedTransactionState::new(&tx, block_index);
            state.set_vm_state(VMState::from_byte(vm_state));
            Ok(TransactionStateRecord::Full(state))
        }
        RECORD_KIND_CONFLICT_STUB => {
            let block_index = reader
                .read_u32()
                .map_err(|e| Error::invalid_data(format!("invalid conflict block index: {e}")))?;
            Ok(TransactionStateRecord::ConflictStub { block_index })
        }
        _ => Err(Error::invalid_data("unknown transaction state record kind")),
    }
}

pub fn serialize_hash_index_state(hash: &UInt256, index: u32) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_bytes(&hash.to_bytes())
        .map_err(|e| Error::serialization(e.to_string()))?;
    writer
        .write_u32(index)
        .map_err(|e| Error::serialization(e.to_string()))?;
    Ok(writer.to_bytes())
}

pub fn deserialize_hash_index_state(bytes: &[u8]) -> Result<HashIndexState> {
    if bytes.len() < 36 {
        return Err(Error::invalid_data(
            "HashIndexState payload is shorter than expected",
        ));
    }

    let hash = UInt256::from_bytes(&bytes[..32])
        .map_err(|e| Error::invalid_data(format!("Invalid hash in HashIndexState: {e}")))?;

    let mut index_bytes = [0u8; 4];
    index_bytes.copy_from_slice(&bytes[32..36]);
    let index = u32::from_le_bytes(index_bytes);

    Ok(HashIndexState::new(hash, index))
}
