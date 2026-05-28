//
// traits.rs - Trait implementations for Transaction
//

use super::*;
use crate::error::CoreError;
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

impl crate::Verifiable for Transaction {
    /// Performs basic structural validation of the transaction.
    ///
    /// # Security Note
    /// This method performs basic structural checks only. For full cryptographic
    /// verification including witness validation, use `verify()` or `verify_state_independent()`
    /// methods on the Transaction struct directly.
    ///
    /// # Checks Performed
    /// - Transaction has at least one signer
    /// - Number of witnesses matches number of signers
    /// - Script is not empty
    /// - Valid fee and validity period
    fn verify(&self) -> bool {
        // Basic structural validation
        // 1. Must have at least one signer
        if self.signers.is_empty() {
            return false;
        }

        // 2. Number of witnesses must match number of signers
        if self.witnesses.len() != self.signers.len() {
            return false;
        }

        // 3. Script must not be empty
        if self.script.is_empty() {
            return false;
        }

        // 4. System fee must be non-negative
        if self.system_fee < 0 {
            return false;
        }

        // 5. Network fee must be non-negative
        if self.network_fee < 0 {
            return false;
        }

        // 6. Valid until block must be reasonable (not zero)
        if self.valid_until_block == 0 {
            return false;
        }

        true
    }

    fn hash(&self) -> CoreResult<UInt256> {
        Transaction::try_hash(self)
    }

    fn hash_data(&self) -> Vec<u8> {
        Transaction::hash_data(self)
    }

    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.signers.iter().map(|s| s.account).collect()
    }

    fn witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
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
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), CoreError> {
        // This operation is not supported for Transaction.
        // The C# implementation throws NotSupportedException.
        Err(CoreError::invalid_operation(
            "FromStackItem is not supported for Transaction",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        StackItem::try_from(self.to_stack_value()?).map_err(|error| {
            CoreError::invalid_operation(format!(
                "Failed to convert transaction StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(Transaction);

// Eq and PartialEq are already derived

impl std::hash::Hash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash_bytes = self.hash().to_bytes();
        StdHash::hash(&hash_bytes, state);
    }
}
