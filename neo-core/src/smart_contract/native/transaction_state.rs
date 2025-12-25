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
                .as_deref()
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
    use crate::network::p2p::payloads::{signer::Signer, transaction::Transaction};
    use crate::smart_contract::BinarySerializer;
    use crate::{smart_contract::IInteroperable, UInt160, Witness, WitnessScope};
    use neo_vm::execution_engine_limits::ExecutionEngineLimits;
    use neo_vm::{op_code::OpCode, StackItem, VMState};

    fn sample_transaction(nonce: u32, network_fee: i64) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(network_fee);
        tx.set_script(vec![OpCode::PUSH1 as u8]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    fn stack_bytes(state: &TransactionState) -> Vec<u8> {
        BinarySerializer::serialize(
            &state.to_stack_item(),
            &ExecutionEngineLimits::default(),
        )
        .expect("serialize stack item")
    }

    fn decode_stack(bytes: &[u8]) -> StackItem {
        BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
            .expect("deserialize stack item")
    }

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
        let mut tx = sample_transaction(7, 0);
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

    #[test]
    fn binary_roundtrip_matches_csharp_transaction_state() {
        let state = TransactionState::new(1, Some(sample_transaction(7, 100)), VMState::NONE);
        let bytes = stack_bytes(&state);

        let mut parsed = TransactionState::new(0, None, VMState::HALT);
        parsed.from_stack_item(decode_stack(&bytes));

        assert_eq!(parsed.block_index, 1);
        assert_eq!(
            parsed.transaction.as_ref().unwrap().hash(),
            state.transaction.as_ref().unwrap().hash()
        );
    }

    #[test]
    fn transaction_state_clone_is_independent() {
        let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
        let mut clone = origin.clone();

        assert_eq!(stack_bytes(&origin), stack_bytes(&clone));

        clone
            .transaction
            .as_mut()
            .expect("clone transaction")
            .set_nonce(2);
        assert_ne!(stack_bytes(&origin), stack_bytes(&clone));
    }

    #[test]
    fn transaction_state_from_replica_updates_fields() {
        let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
        let mut replica = TransactionState::new(0, None, VMState::HALT);
        replica.from_replica(&origin);

        assert_eq!(stack_bytes(&replica), stack_bytes(&origin));
        assert_eq!(
            replica.transaction.as_ref().unwrap().nonce(),
            origin.transaction.as_ref().unwrap().nonce()
        );

        let new_origin =
            TransactionState::new(2, Some(sample_transaction(99, 200)), VMState::NONE);
        replica.from_replica(&new_origin);

        assert_eq!(stack_bytes(&replica), stack_bytes(&new_origin));
        assert_eq!(replica.block_index, 2);
        assert_eq!(
            replica.transaction.as_ref().unwrap().network_fee(),
            new_origin.transaction.as_ref().unwrap().network_fee()
        );
    }

    #[test]
    fn trimmed_transaction_state_roundtrip_via_binary_serializer() {
        let state = TransactionState::new(7, None, VMState::NONE);
        let bytes = stack_bytes(&state);

        let mut parsed = TransactionState::new(0, Some(sample_transaction(1, 0)), VMState::HALT);
        parsed.from_stack_item(decode_stack(&bytes));

        assert_eq!(parsed.block_index, 7);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.state, VMState::NONE);
    }
}
