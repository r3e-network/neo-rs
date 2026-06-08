//
// traits.rs - Trait implementations for Transaction
//

use super::*;
use neo_error::CoreError;
use neo_primitives::error::PrimitiveResult;
use neo_primitives::SerializablePayload;
use neo_vm_rs::StackValue;

impl SerializablePayload for Transaction {
    fn hash_data(&self) -> Vec<u8> {
        Transaction::hash_data(self)
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
    /// Converts the transaction to a neo-vm-rs stack value (matches C# `Transaction.ToStackItem` layout).
    pub fn to_stack_value(&self) -> Result<StackValue, CoreError> {
        let sender = self
            .sender()
            .ok_or_else(|| {
                CoreError::invalid_argument("Sender is not specified in the transaction")
            })?
            .to_bytes();

        Ok(StackValue::Array(vec![
            StackValue::ByteString(self.try_hash()?.to_bytes()),
            StackValue::Integer(i64::from(self.version)),
            StackValue::Integer(i64::from(self.nonce)),
            StackValue::ByteString(sender),
            StackValue::Integer(self.system_fee),
            StackValue::Integer(self.network_fee),
            StackValue::Integer(i64::from(self.valid_until_block)),
            StackValue::ByteString(self.script.clone()),
        ]))
    }
}

impl Interoperable for Transaction {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), neo_vm::VmError> {
        // This operation is not supported for Transaction.
        // The C# implementation throws NotSupportedException.
        Err(neo_vm::VmError::invalid_operation_msg(
            "FromStackItem is not supported for Transaction",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, neo_vm::VmError> {
        // Build the lean neo-vm-rs StackValue projection first, then adapt it to
        // the host StackItem. Both steps' errors surface as a single VM error.
        let to_item = || -> Result<StackItem, CoreError> {
            Ok(StackItem::try_from(self.to_stack_value()?)?)
        };
        to_item().map_err(|error| {
            neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert transaction StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
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
