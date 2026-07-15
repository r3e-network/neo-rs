use neo_error::{CoreError, CoreResult};
use neo_payloads::{Transaction, TrimmedBlock};
use neo_primitives::UInt256;
use neo_serialization::BinarySerializer;
use neo_vm::{StackItem, VmState as VMState};

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

    pub(crate) fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.hash.to_bytes()),
            StackItem::from_i64(i64::from(self.index)),
        ])
    }

    pub(crate) fn from_stack_item(stack_item: &StackItem) -> CoreResult<Self> {
        let decoder = crate::support::codec::StructDecoder::new(stack_item, "HashIndexState")?;
        if decoder.len() < 2 {
            return Err(CoreError::invalid_data(
                "HashIndexState struct is shorter than expected",
            ));
        }
        let hash = decoder.hash256(0, "hash")?;
        let index = decoder.u32(1, "index")?;
        Ok(Self { hash, index })
    }
}

neo_vm::impl_interoperable_via_stack_item!(HashIndexState);

impl LedgerContract {
    /// Serialises a `(hash, index)` pair into the C# `HashIndexState`
    /// wire format used for the current-block pointer.
    pub fn serialize_hash_index_state(&self, hash: &UInt256, index: u32) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(
            &HashIndexState::new(*hash, index),
            "HashIndexState",
        )
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
            .try_to_stack_item()?;
        BinarySerializer::serialize_default(&item)
            .map_err(|e| CoreError::serialization(format!("TransactionState: {e}")))
    }

    /// Serialises a conflict-stub record into the C# wire format:
    /// `Struct[Integer(BlockIndex)]` with a null transaction.
    pub fn serialize_conflict_stub(&self, block_index: u32) -> CoreResult<Vec<u8>> {
        let item = neo_payloads::TransactionState::new(block_index, None, VMState::NONE)
            .try_to_stack_item()?;
        BinarySerializer::serialize_default(&item)
            .map_err(|e| CoreError::serialization(format!("TransactionState stub: {e}")))
    }

    pub(crate) fn transaction_to_bytes(tx: &Transaction, method: &str) -> CoreResult<Vec<u8>> {
        let item = tx.to_stack_item().map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: stack item: {e}"))
        })?;
        BinarySerializer::serialize_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn signers_to_bytes(
        signers: &[neo_payloads::Signer],
        method: &str,
    ) -> CoreResult<Vec<u8>> {
        let item = StackItem::from_array(
            signers
                .iter()
                .map(neo_payloads::Signer::to_stack_item)
                .collect::<Vec<_>>(),
        );
        BinarySerializer::serialize_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn trimmed_block_to_bytes(
        block: &TrimmedBlock,
        method: &str,
    ) -> CoreResult<Vec<u8>> {
        let item = block.to_stack_item();
        BinarySerializer::serialize_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("LedgerContract::{method}: serialize: {e}"))
        })
    }

    pub(crate) fn deserialize_hash_index_state(bytes: &[u8]) -> CoreResult<(UInt256, u32)> {
        let item = crate::support::codec::decode_stack_item(bytes, "HashIndexState")?;
        let state = HashIndexState::from_stack_item(&item)
            .map_err(|e| CoreError::invalid_data(format!("HashIndexState: {e}")))?;
        Ok((state.hash, state.index))
    }

    /// Decodes the C#-compatible value stored under `Prefix_Transaction`.
    ///
    /// The value is a C# `TransactionState` interoperable stack item:
    /// `Struct[Integer]` is a conflict stub, while
    /// `Struct[Integer, ByteString, Integer]` is a full record.
    ///
    /// Storage and static-file ledger providers share this codec so moving an
    /// immutable record between physical tiers cannot change its semantics.
    pub fn decode_transaction_state(bytes: &[u8]) -> CoreResult<neo_payloads::TransactionState> {
        let item = crate::support::codec::decode_stack_item(bytes, "TransactionState")?;
        let mut state = neo_payloads::TransactionState::new(0, None, VMState::NONE);
        state
            .from_stack_item(item)
            .map_err(|e| CoreError::invalid_data(format!("TransactionState: {e}")))?;
        Ok(state)
    }
}
