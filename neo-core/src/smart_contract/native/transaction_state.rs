//! TransactionState - matches C# Neo.SmartContract.Native.TransactionState exactly.

use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::network::p2p::payloads::transaction::Transaction;
use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::{StackItem, VMState};
use num_traits::ToPrimitive;
use tracing::warn;

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

    fn deserialize_transaction(bytes: &[u8]) -> Option<Transaction> {
        let mut reader = MemoryReader::new(bytes);
        Transaction::deserialize(&mut reader).ok()
    }

    fn serialize_transaction(tx: &Transaction) -> Option<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        tx.serialize(&mut writer).ok()?;
        Some(writer.into_bytes())
    }

    fn decode_vm_state(value: u8) -> VMState {
        match value {
            x if x == VMState::HALT as u8 => VMState::HALT,
            x if x == VMState::FAULT as u8 => VMState::FAULT,
            x if x == VMState::BREAK as u8 => VMState::BREAK,
            _ => VMState::NONE,
        }
    }
}

impl IInteroperable for TransactionState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.is_empty() {
                return;
            }

            if let Ok(integer) = items[0].as_int() {
                if let Some(index) = integer.to_u32() {
                    self.block_index = index;
                }
            }

            // Conflict-only representations encode only the block index.
            if items.len() == 1 {
                self.transaction = None;
                self.state = VMState::NONE;
                return;
            }

            self.transaction = items[1]
                .as_bytes()
                .ok()
                .and_then(Self::deserialize_transaction);

            self.state = items
                .get(2)
                .and_then(|item| item.as_int().ok())
                .and_then(|value| value.to_u8())
                .map(Self::decode_vm_state)
                .unwrap_or(VMState::NONE);
        }
    }

    fn to_stack_item(&self) -> StackItem {
        if let Some(tx) = &self.transaction {
            if let Some(bytes) = Self::serialize_transaction(tx) {
                return StackItem::from_struct(vec![
                    StackItem::from_int(self.block_index),
                    StackItem::from_byte_string(bytes),
                    StackItem::from_int(self.state as u8),
                ]);
            }

            warn!(
                target: "neo",
                block_index = self.block_index,
                "failed to serialize transaction for TransactionState stack item; emitting conflict-only representation"
            );
        }

        StackItem::from_struct(vec![StackItem::from_int(self.block_index)])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionState;
    use crate::network::p2p::payloads::transaction::Transaction;
    use neo_vm::VMState;

    #[test]
    fn conflict_stub_roundtrip() {
        let state = TransactionState::new(7, None, VMState::NONE);
        let stack = state.to_stack_item();

        let mut parsed = TransactionState::new(0, None, VMState::HALT);
        parsed.from_stack_item(stack);

        assert_eq!(parsed.block_index, 7);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.state, VMState::NONE);
    }

    #[test]
    fn full_roundtrip_preserves_transaction_and_state() {
        let mut tx = Transaction::new();
        tx.set_script(vec![0x01, 0x02, 0x03]);

        let state = TransactionState::new(42, Some(tx.clone()), VMState::HALT);
        let stack = state.to_stack_item();

        let mut parsed = TransactionState::new(0, None, VMState::NONE);
        parsed.from_stack_item(stack);

        assert_eq!(parsed.block_index, 42);
        assert_eq!(parsed.state, VMState::HALT);
        assert!(parsed.transaction.is_some());
        assert_eq!(parsed.transaction.unwrap().hash(), tx.hash());
    }
}
