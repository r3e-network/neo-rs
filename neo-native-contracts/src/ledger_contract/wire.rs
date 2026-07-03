use neo_error::{CoreError, CoreResult};
use neo_payloads::{Transaction, TrimmedBlock};
use neo_primitives::UInt256;
use neo_serialization::BinarySerializer;
use neo_vm_rs::{ExecutionEngineLimits, StackValue, VmState as VMState};

use super::LedgerContract;

/// C# `HashIndexState`: the current-block pointer persisted as an
/// interoperable `Struct[Hash.ToArray(), Index]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HashIndexState {
    pub(crate) hash: UInt256,
    pub(crate) index: u32,
}

impl HashIndexState {
    pub(crate) fn new(hash: UInt256, index: u32) -> Self {
        Self { hash, index }
    }

    pub(crate) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(self.hash.to_bytes()),
                StackValue::Integer(i64::from(self.index)),
            ],
        )
    }

    pub(crate) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "HashIndexState record is not a Struct stack item",
            ));
        };
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "HashIndexState struct is shorter than expected",
            ));
        }

        let hash_bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("HashIndexState hash is not byte-like"))?;
        let hash = crate::args::bytes_to_hash256(&hash_bytes, "HashIndexState hash")?;
        let index = neo_vm_rs::stack_value_as_u32(&items[1])
            .ok_or_else(|| CoreError::invalid_data("HashIndexState index out of uint range"))?;
        Ok(Self { hash, index })
    }
}

neo_vm::impl_interoperable_via_stack_value!(HashIndexState);

impl LedgerContract {
    /// Serialises a `(hash, index)` pair into the C# `HashIndexState`
    /// wire format used for the current-block pointer.
    pub fn serialize_hash_index_state(&self, hash: &UInt256, index: u32) -> CoreResult<Vec<u8>> {
        let item = HashIndexState::new(*hash, index).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("HashIndexState: {e}")))
    }

    /// Serialises a persisted transaction state into the C# wire format:
    /// `Struct[Integer(BlockIndex), ByteString(tx bytes), Integer((byte)State)]`.
    pub fn serialize_persisted_transaction_state(
        &self,
        block_index: u32,
        vm_state: VMState,
        tx: &Transaction,
    ) -> CoreResult<Vec<u8>> {
        let item = neo_payloads::TransactionState::new(block_index, Some(tx.clone()), vm_state)
            .try_to_stack_value()?;
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("TransactionState: {e}")))
    }

    /// Serialises a conflict-stub record into the C# wire format:
    /// `Struct[Integer(BlockIndex)]` with a null transaction.
    pub fn serialize_conflict_stub(&self, block_index: u32) -> CoreResult<Vec<u8>> {
        let item =
            neo_payloads::TransactionState::new(block_index, None, VMState::NONE).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("TransactionState stub: {e}")))
    }

    pub(crate) fn transaction_to_bytes(tx: &Transaction, method: &str) -> CoreResult<Vec<u8>> {
        let item = tx.to_stack_value().map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: stack value: {e}"))
        })?;
        BinarySerializer::serialize_stack_value_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn signers_to_bytes(
        signers: &[neo_payloads::Signer],
        method: &str,
    ) -> CoreResult<Vec<u8>> {
        let item = StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            signers
                .iter()
                .map(neo_payloads::Signer::to_stack_value)
                .collect::<Vec<_>>(),
        );
        BinarySerializer::serialize_stack_value_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn trimmed_block_to_bytes(
        block: &TrimmedBlock,
        method: &str,
    ) -> CoreResult<Vec<u8>> {
        let item = block.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn deserialize_hash_index_state(bytes: &[u8]) -> CoreResult<(UInt256, u32)> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::invalid_data(format!("HashIndexState: {e}")))?;
        let state = HashIndexState::from_stack_value(item)
            .map_err(|e| CoreError::invalid_data(format!("HashIndexState: {e}")))?;
        Ok((state.hash, state.index))
    }

    /// Decodes a `Prefix_Transaction` record: the C# `TransactionState`
    /// interoperable stack item. `Struct[Integer]` is a conflict stub;
    /// `Struct[Integer, ByteString, Integer]` is a full record.
    pub(crate) fn decode_transaction_state(
        bytes: &[u8],
    ) -> CoreResult<neo_payloads::TransactionState> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::invalid_data(format!("TransactionState: {e}")))?;
        let mut state = neo_payloads::TransactionState::new(0, None, VMState::NONE);
        state
            .from_stack_value(item)
            .map_err(|e| CoreError::invalid_data(format!("TransactionState: {e}")))?;
        Ok(state)
    }
}
