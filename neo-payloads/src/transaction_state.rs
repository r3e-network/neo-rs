//! TransactionState - matches C# Neo.SmartContract.Native.TransactionState exactly.

use crate::Transaction;
use neo_error::CoreError;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_vm::{Interoperable, InteroperableError};
use neo_vm_rs::{StackValue, VmState as VMState};

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

    /// Converts to a neo-vm-rs stack value, preserving C# failure semantics:
    /// a full transaction record must either contain the serialized
    /// transaction bytes or fail, never silently degrade to a conflict stub.
    pub fn try_to_stack_value(&self) -> Result<StackValue, CoreError> {
        if let Some(tx) = &self.transaction {
            return Ok(StackValue::Struct(
                0,
                vec![
                    StackValue::Integer(i64::from(self.block_index)),
                    StackValue::ByteString(Self::serialize_transaction(tx)?),
                    StackValue::Integer(i64::from(self.state.to_byte())),
                ],
            ));
        }

        Ok(StackValue::Struct(
            0,
            vec![StackValue::Integer(i64::from(self.block_index))],
        ))
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        self.try_to_stack_value()
            .expect("TransactionState stack projection should serialize valid transactions")
    }

    /// Updates this transaction state from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "TransactionState record is not a Struct stack item",
            ));
        };
        if items.is_empty() {
            return Err(CoreError::invalid_data("TransactionState struct is empty"));
        }

        self.block_index = neo_vm_rs::stack_value_as_u32(&items[0]).ok_or_else(|| {
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

        let tx_bytes = items[1]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("TransactionState transaction is not bytes"))?;
        self.transaction = Some(Self::deserialize_transaction(&tx_bytes)?);
        let state_byte = neo_vm_rs::stack_value_as_u8(&items[2])
            .ok_or_else(|| CoreError::invalid_data("TransactionState VMState out of byte range"))?;
        self.state = Self::decode_vm_state(state_byte);
        Ok(())
    }
}

impl Interoperable for TransactionState {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        self.from_stack_value(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        self.try_to_stack_value()
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionState;
    use crate::Witness;
    use crate::{signer::Signer, transaction::Transaction};
    use neo_primitives::UInt160;
    use neo_primitives::WitnessScope;
    use neo_serialization::BinarySerializer;
    use neo_vm::Interoperable;
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
            StackValue::Struct(0, vec![StackValue::Integer(7)])
        );
    }

    #[test]
    fn transaction_state_reads_from_neo_vm_rs_stack_value() {
        let tx = sample_transaction(99, 200);
        let tx_bytes = TransactionState::serialize_transaction(&tx).unwrap();
        let mut state = TransactionState::new(0, None, VMState::NONE);

        state
            .from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::Integer(11),
                    StackValue::ByteString(tx_bytes),
                    StackValue::Integer(VMState::HALT.to_byte() as i64),
                ],
            ))
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
    fn interoperable_projection_matches_stack_value_projection() {
        let state = TransactionState::new(12, Some(sample_transaction(7, 100)), VMState::HALT);
        let expected = state.to_stack_value();

        assert_eq!(Interoperable::to_stack_value(&state).unwrap(), expected);

        let mut parsed = TransactionState::new(0, None, VMState::NONE);
        Interoperable::from_stack_value(&mut parsed, expected).unwrap();

        assert_eq!(parsed.block_index, 12);
        assert_eq!(parsed.state, VMState::HALT);
        assert_eq!(
            parsed.transaction.as_ref().unwrap().hash(),
            state.transaction.as_ref().unwrap().hash()
        );
    }

    #[test]
    fn interoperable_projection_accepts_conflict_stub() {
        let state = TransactionState::new(9, None, VMState::NONE);
        let stack_value = Interoperable::to_stack_value(&state).unwrap();

        let mut parsed = TransactionState::new(0, Some(sample_transaction(1, 0)), VMState::HALT);
        Interoperable::from_stack_value(&mut parsed, stack_value).unwrap();

        assert_eq!(parsed.block_index, 9);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.state, VMState::NONE);
    }

    #[test]
    fn transaction_state_rejects_invalid_stack_shapes() {
        let mut parsed = TransactionState::new(0, None, VMState::NONE);

        assert!(
            parsed
                .from_stack_value(StackValue::Array(0, vec![]))
                .is_err()
        );
        assert!(
            parsed
                .from_stack_value(StackValue::Struct(0, vec![]))
                .is_err()
        );
        assert!(
            parsed
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![StackValue::Integer(7), StackValue::ByteString(vec![])]
                ))
                .is_err()
        );
    }

    #[test]
    fn transaction_state_rejects_malformed_transaction_bytes() {
        let mut parsed = TransactionState::new(0, None, VMState::NONE);

        let error = parsed
            .from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::Integer(7),
                    StackValue::ByteString(vec![0xff]),
                    StackValue::Integer(VMState::HALT.to_byte() as i64),
                ],
            ))
            .unwrap_err();

        assert!(
            error.to_string().contains("TransactionState transaction"),
            "{error}"
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
