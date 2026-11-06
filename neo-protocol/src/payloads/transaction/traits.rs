// Protocol trait implementations for Transaction.
use super::Transaction;
use crate::witness::{Witness, WitnessData};
use neo_primitives::error::{PrimitiveError, PrimitiveResult};
use neo_primitives::{Inventory, InventoryType, SerializablePayload, UInt256};
use std::any::Any;
use std::hash::{Hash as StdHash, Hasher};

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
impl neo_primitives::Verifiable for Transaction {
    // Only the existing structural check, not VM/cryptographic validation.
    fn verify(&self) -> bool {
        if self.signers.is_empty() {
            return false;
        }
        if self.witnesses.len() != self.signers.len() {
            return false;
        }
        if self.script.is_empty() {
            return false;
        }
        if self.system_fee < 0 {
            return false;
        }
        if self.network_fee < 0 {
            return false;
        }
        if self.valid_until_block == 0 {
            return false;
        }
        true
    }
    fn hash(&self) -> PrimitiveResult<UInt256> {
        // The old CoreError conversion added this prefix before PrimitiveError.
        Transaction::try_hash(self)
            .map_err(|e| PrimitiveError::invalid_data(format!("Codec error: {e}")))
    }
    fn hash_data(&self) -> Vec<u8> {
        Transaction::hash_data(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl WitnessData for Transaction {
    fn witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }
    // Preserve historical mutable access without strengthening invalidation.
    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }
}
impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}
impl std::hash::Hash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash_bytes = self.hash().to_bytes();
        StdHash::hash(&hash_bytes, state);
    }
}
