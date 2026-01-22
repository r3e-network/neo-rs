//
// traits.rs - Trait implementations for Transaction
//

use super::*;

impl IInventory for Transaction {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Transaction
    }

    fn hash(&mut self) -> UInt256 {
        Transaction::hash(self)
    }
}

impl crate::IVerifiable for Transaction {
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
        Ok(Transaction::hash(self))
    }

    fn get_hash_data(&self) -> Vec<u8> {
        Transaction::get_hash_data(self)
    }

    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.signers.iter().map(|s| s.account).collect()
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IInteroperable for Transaction {
    fn from_stack_item(&mut self, _stack_item: StackItem) {
        // This operation is not supported for Transaction.
        // The C# implementation throws NotSupportedException.
        panic!("NotSupportedException: Transaction::from_stack_item is not supported");
    }

    fn to_stack_item(&self) -> StackItem {
        if self.signers.is_empty() {
            panic!("ArgumentException: Sender is not specified in the transaction.");
        }
        let sender = self.signers[0].account.to_bytes();

        StackItem::from_array(vec![
            StackItem::from_byte_string(self.hash().to_bytes()),
            StackItem::from_int(self.version as i64),
            StackItem::from_int(self.nonce),
            StackItem::from_byte_string(sender),
            StackItem::from_int(self.system_fee),
            StackItem::from_int(self.network_fee),
            StackItem::from_int(self.valid_until_block),
            StackItem::from_byte_string(self.script.clone()),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
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
