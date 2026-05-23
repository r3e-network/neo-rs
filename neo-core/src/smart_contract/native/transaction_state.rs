//! TransactionState - matches C# Neo.SmartContract.Native.TransactionState exactly.

use crate::error::CoreError;
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::network::p2p::payloads::transaction::Transaction;
use neo_vm_rs::{StackValue, VmState as VMState};
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
        VMState::from_byte(value)
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        if let Some(tx) = &self.transaction {
            if let Some(bytes) = Self::serialize_transaction(tx) {
                return StackValue::Struct(vec![
                    StackValue::Integer(i64::from(self.block_index)),
                    StackValue::ByteString(bytes),
                    StackValue::Integer(i64::from(self.state.to_byte())),
                ]);
            }

            warn!(
                target: "neo",
                block_index = self.block_index,
                "failed to serialize transaction for TransactionState stack value; emitting conflict-only representation"
            );
        }

        StackValue::Struct(vec![StackValue::Integer(i64::from(self.block_index))])
    }

    /// Updates this transaction state from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if items.is_empty() {
                return Ok(());
            }

            if let Some(index) = items[0]
                .to_i128()
                .and_then(|value| u32::try_from(value).ok())
            {
                self.block_index = index;
            }

            // Conflict-only representations encode only the block index.
            if items.len() == 1 {
                self.transaction = None;
                self.state = VMState::NONE;
                return Ok(());
            }

            self.transaction = items[1]
                .to_byte_string_bytes()
                .as_deref()
                .and_then(Self::deserialize_transaction);

            self.state = items
                .get(2)
                .and_then(|item| item.to_i128())
                .and_then(|value| u8::try_from(value).ok())
                .map(Self::decode_vm_state)
                .unwrap_or(VMState::NONE);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionState;
    use crate::network::p2p::payloads::{signer::Signer, transaction::Transaction};
    use crate::smart_contract::BinarySerializer;
    use crate::{UInt160, Witness, WitnessScope};
    use neo_vm_rs::{ExecutionEngineLimits, OpCode, StackValue, VmState as VMState};

    fn sample_transaction(nonce: u32, network_fee: i64) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(network_fee);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    fn stack_bytes(state: &TransactionState) -> Vec<u8> {
        BinarySerializer::serialize_stack_value(
            &state.to_stack_value(),
            &ExecutionEngineLimits::default(),
        )
        .expect("serialize stack item")
    }

    fn decode_stack_value(bytes: &[u8]) -> StackValue {
        BinarySerializer::deserialize_stack_value(bytes).expect("deserialize stack value")
    }

    #[test]
    fn transaction_state_projects_conflict_stub_to_neo_vm_rs_stack_value() {
        let state = TransactionState::new(7, None, VMState::NONE);

        assert_eq!(
            state.to_stack_value(),
            StackValue::Struct(vec![StackValue::Integer(7)])
        );
    }

    #[test]
    fn transaction_state_reads_from_neo_vm_rs_stack_value() {
        let tx = sample_transaction(99, 200);
        let tx_bytes = TransactionState::serialize_transaction(&tx).unwrap();
        let mut state = TransactionState::new(0, None, VMState::NONE);

        state
            .from_stack_value(StackValue::Struct(vec![
                StackValue::Integer(11),
                StackValue::ByteString(tx_bytes),
                StackValue::Integer(VMState::HALT.to_byte() as i64),
            ]))
            .unwrap();

        assert_eq!(state.block_index, 11);
        assert_eq!(state.state, VMState::HALT);
        assert_eq!(state.transaction.as_ref().unwrap().hash(), tx.hash());
    }

    #[test]
    fn conflict_stub_roundtrip() {
        let state = TransactionState::new(7, None, VMState::NONE);
        let value = state.to_stack_value();

        let mut parsed = TransactionState::new(0, None, VMState::HALT);
        parsed.from_stack_value(value).unwrap();

        assert_eq!(parsed.block_index, 7);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.state, VMState::NONE);
    }

    #[test]
    fn full_roundtrip_preserves_transaction_and_state() {
        let mut tx = sample_transaction(7, 0);
        tx.set_script(vec![0x01, 0x02, 0x03]);

        let state = TransactionState::new(42, Some(tx.clone()), VMState::HALT);
        let value = state.to_stack_value();

        let mut parsed = TransactionState::new(0, None, VMState::NONE);
        parsed.from_stack_value(value).unwrap();

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
        parsed.from_stack_value(decode_stack_value(&bytes)).unwrap();

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
    fn transaction_state_from_stack_value_updates_fields() {
        let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
        let mut replica = TransactionState::new(0, None, VMState::HALT);
        replica.from_stack_value(origin.to_stack_value()).unwrap();

        assert_eq!(stack_bytes(&replica), stack_bytes(&origin));
        assert_eq!(
            replica.transaction.as_ref().unwrap().nonce(),
            origin.transaction.as_ref().unwrap().nonce()
        );

        let new_origin = TransactionState::new(2, Some(sample_transaction(99, 200)), VMState::NONE);
        replica
            .from_stack_value(new_origin.to_stack_value())
            .unwrap();

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
        parsed.from_stack_value(decode_stack_value(&bytes)).unwrap();

        assert_eq!(parsed.block_index, 7);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.state, VMState::NONE);
    }
}
