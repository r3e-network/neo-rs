//
// traits.rs - Trait implementations for Transaction
//

use super::*;
use neo_error::CoreError;
use neo_primitives::SerializablePayload;
use neo_vm::InteroperableError;
use neo_vm::StackItem;

impl SerializablePayload for Transaction {
    fn hash_data(&self) -> Vec<u8> {
        Transaction::hash_data(self)
    }

    fn hash(&self) -> UInt256 {
        Transaction::hash(self)
    }

    fn witness_count(&self) -> usize {
        self.witnesses.len()
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        self.witnesses
            .get(index)
            .map(|w| w.invocation_script.as_slice())
            .unwrap_or_default()
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        self.witnesses
            .get(index)
            .map(|w| w.verification_script.as_slice())
            .unwrap_or_default()
    }
}

impl Inventory for Transaction {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Transaction
    }
}

impl Transaction {
    /// Converts the transaction to a neo-vm stack item (matches C# `Transaction.ToStackItem` layout).
    pub fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        let sender = self
            .sender()
            .ok_or_else(|| {
                CoreError::invalid_argument("Sender is not specified in the transaction")
            })?
            .to_bytes();

        Ok(StackItem::from_array(vec![
            StackItem::from_byte_string(self.try_hash()?.to_bytes()),
            StackItem::from_i64(i64::from(self.version)),
            StackItem::from_i64(i64::from(self.nonce)),
            StackItem::from_byte_string(sender),
            StackItem::from_i64(self.system_fee),
            StackItem::from_i64(self.network_fee),
            StackItem::from_i64(i64::from(self.valid_until_block)),
            StackItem::from_byte_string(self.script.clone()),
        ]))
    }
}

impl Interoperable for Transaction {
    fn from_stack_item(&mut self, _value: StackItem) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "Transaction::from_stack_item is not supported".into(),
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Transaction::to_stack_item(self).map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }
}

// Use macro to reduce boilerplate
neo_io::impl_default_via_new!(Transaction);

// Eq and PartialEq are already derived

impl std::hash::Hash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash_bytes = self.hash().to_bytes();
        StdHash::hash(&hash_bytes, state);
    }
}
