//! TransactionState - matches C# Neo.SmartContract.Native.TransactionState exactly.

use crate::Transaction;
use neo_error::CoreError;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_vm::{Interoperable, InteroperableError};
use neo_vm::{StackItem, VmState as VMState};
use num_traits::ToPrimitive;

/// State of a transaction in the ledger (matches C# TransactionState).
#[derive(Clone, Debug)]
pub struct TransactionState {
    /// The block index containing the transaction.
    pub block_index: u32,

    /// The transaction itself; `None` when only conflict metadata is available.
    pub transaction: Option<Transaction>,

    /// The execution state (mirrors the neo-vm [`VMState`] enum).
    pub state: VMState,
}

impl TransactionState {
    /// Creates a new transaction state.
    pub fn new(block_index: u32, transaction: Option<Transaction>, state: VMState) -> Self {
        Self {
            block_index,
            transaction,
            state,
        }
    }

    /// Returns the block index of the transaction.
    pub fn block_index(&self) -> u32 {
        self.block_index
    }

    fn deserialize_transaction(bytes: &[u8]) -> Result<Transaction, CoreError> {
        let mut reader = MemoryReader::new(bytes);
        let transaction = Transaction::deserialize(&mut reader)
            .map_err(|e| CoreError::invalid_data(format!("TransactionState transaction: {e}")))?;
        if reader.remaining() != 0 {
            return Err(CoreError::invalid_data(
                "TransactionState transaction has trailing bytes",
            ));
        }
        Ok(transaction)
    }

    fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>, CoreError> {
        let mut writer = BinaryWriter::new();
        tx.serialize(&mut writer)
            .map_err(|e| CoreError::serialization(format!("TransactionState transaction: {e}")))?;
        Ok(writer.into_bytes())
    }

    fn decode_vm_state(value: u8) -> VMState {
        VMState::from_byte(value)
    }

    /// Converts to a neo-vm stack item, preserving C# failure semantics:
    /// a full transaction record must either contain the serialized
    /// transaction bytes or fail, never silently degrade to a conflict stub.
    pub fn try_to_stack_item(&self) -> Result<StackItem, CoreError> {
        if let Some(tx) = &self.transaction {
            return Ok(StackItem::from_struct(vec![
                StackItem::from_i64(i64::from(self.block_index)),
                StackItem::from_byte_string(Self::serialize_transaction(tx)?),
                StackItem::from_i64(i64::from(self.state.to_byte())),
            ]));
        }

        Ok(StackItem::from_struct(vec![StackItem::from_i64(
            i64::from(self.block_index),
        )]))
    }

    /// Updates this transaction state from a neo-vm stack item.
    pub fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let StackItem::Struct(structure) = stack_item else {
            return Err(CoreError::invalid_data(
                "TransactionState record is not a Struct stack item",
            ));
        };
        let items = structure.items();
        if items.is_empty() {
            return Err(CoreError::invalid_data("TransactionState struct is empty"));
        }

        self.block_index = items[0]
            .as_int()
            .ok()
            .and_then(|value| value.to_u32())
            .ok_or_else(|| {
                CoreError::invalid_data("TransactionState block index out of uint range")
            })?;

        // Conflict-only representations encode only the block index.
        if items.len() == 1 {
            self.transaction = None;
            self.state = VMState::NONE;
            return Ok(());
        }
        if items.len() < 3 {
            return Err(CoreError::invalid_data(
                "TransactionState struct is shorter than expected",
            ));
        }

        let tx_bytes = match &items[1] {
            StackItem::ByteString(bytes) => bytes.clone(),
            StackItem::Buffer(buffer) => buffer.data(),
            _ => {
                return Err(CoreError::invalid_data(
                    "TransactionState transaction is not bytes",
                ));
            }
        };
        self.transaction = Some(Self::deserialize_transaction(&tx_bytes)?);
        let state_byte = items[2]
            .as_int()
            .ok()
            .and_then(|value| value.to_u8())
            .ok_or_else(|| CoreError::invalid_data("TransactionState VMState out of byte range"))?;
        self.state = Self::decode_vm_state(state_byte);
        Ok(())
    }
}

impl Interoperable for TransactionState {
    fn from_stack_item(&mut self, value: StackItem) -> Result<(), InteroperableError> {
        TransactionState::from_stack_item(self, value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        self.try_to_stack_item()
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }
}

#[cfg(test)]
#[path = "../tests/ledger/transaction_state.rs"]
mod tests;
