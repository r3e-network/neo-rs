//! Ledger contract state types and serialization helpers.
use crate::UInt256;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::network::p2p::payloads::transaction::{MAX_TRANSACTION_SIZE, Transaction};
use crate::smart_contract::native::{
    hash_index_state::HashIndexState, trimmed_block::TrimmedBlock,
};
use neo_vm::vm_state::VMState;
use serde::{Deserialize, Serialize};

const RECORD_KIND_TRANSACTION: u8 = 0x01;
const RECORD_KIND_CONFLICT_STUB: u8 = 0x02;

fn vm_state_from_raw(value: u8) -> VMState {
    match value {
        value if value == VMState::HALT as u8 => VMState::HALT,
        value if value == VMState::FAULT as u8 => VMState::FAULT,
        value if value == VMState::BREAK as u8 => VMState::BREAK,
        _ => VMState::NONE,
    }
}

fn parse_uint256_invalid_data(bytes: &[u8], name: &str) -> Result<UInt256> {
    UInt256::from_bytes(bytes).map_err(|e| Error::invalid_data(format!("Invalid {name}: {e}")))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PersistedTransactionState {
    block_index: u32,
    vm_state: u8,
    transaction: Transaction,
}

impl PersistedTransactionState {
    pub fn new(tx: &Transaction, block_index: u32) -> Self {
        Self {
            block_index,
            vm_state: VMState::NONE as u8,
            transaction: tx.clone(),
        }
    }

    pub fn block_index(&self) -> u32 {
        self.block_index
    }

    pub fn vm_state_raw(&self) -> u8 {
        self.vm_state
    }

    pub fn vm_state(&self) -> VMState {
        vm_state_from_raw(self.vm_state)
    }

    pub fn set_vm_state(&mut self, vm_state: VMState) {
        self.vm_state = vm_state as u8;
    }

    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    pub fn transaction_mut(&mut self) -> &mut Transaction {
        &mut self.transaction
    }

    pub fn transaction_hash(&self) -> UInt256 {
        self.transaction.hash()
    }
}

#[derive(Clone, Default)]
pub struct LedgerTransactionStates {
    states: Vec<PersistedTransactionState>,
}

impl LedgerTransactionStates {
    pub fn new(states: Vec<PersistedTransactionState>) -> Self {
        Self { states }
    }

    pub fn states(&self) -> &[PersistedTransactionState] {
        &self.states
    }

    pub fn mark_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        for state in &mut self.states {
            if &state.transaction_hash() == hash {
                state.set_vm_state(vm_state);
                return true;
            }
        }
        false
    }

    pub fn into_updates(self) -> Vec<(UInt256, VMState)> {
        self.states
            .into_iter()
            .map(|state| (state.transaction_hash(), state.vm_state()))
            .collect()
    }
}

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
            state.set_vm_state(vm_state_from_raw(vm_state));
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

    let hash = parse_uint256_invalid_data(&bytes[..32], "hash in HashIndexState")?;

    let mut index_bytes = [0u8; 4];
    index_bytes.copy_from_slice(&bytes[32..36]);
    let index = u32::from_le_bytes(index_bytes);

    Ok(HashIndexState::new(hash, index))
}
