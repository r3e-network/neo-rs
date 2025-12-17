//! Storage key conversion between neo-storage and neo-state formats.
//!
//! neo-storage uses contract ID (i32), while neo-state uses contract hash (UInt160).
//! This module provides conversion utilities for state root calculation.

use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, NeoToken, OracleContract, PolicyContract,
    RoleManagement,
};
use neo_core::UInt160;
use neo_state::{StorageItem, StorageKey};
use std::collections::HashMap;

/// Converts storage keys between neo-storage (ID-based) and neo-state (hash-based) formats.
#[allow(dead_code)] // Will be used when full state root calculation is implemented
pub struct StorageKeyConverter {
    /// Mapping from contract ID to contract hash.
    id_to_hash: HashMap<i32, UInt160>,
    /// Mapping from contract hash to contract ID.
    hash_to_id: HashMap<UInt160, i32>,
}

#[allow(dead_code)] // Methods will be used when full state root calculation is implemented
impl StorageKeyConverter {
    /// Native contract IDs (matching C# Neo implementation).
    pub const LEDGER_CONTRACT_ID: i32 = -1;
    pub const NEO_TOKEN_ID: i32 = -2;
    pub const GAS_TOKEN_ID: i32 = -3;
    pub const POLICY_CONTRACT_ID: i32 = -4;
    pub const ROLE_MANAGEMENT_ID: i32 = -5;
    pub const ORACLE_CONTRACT_ID: i32 = -6;
    pub const CONTRACT_MANAGEMENT_ID: i32 = -7;
    pub const NOTARY_CONTRACT_ID: i32 = -8;

    /// Creates a new converter with native contract mappings.
    pub fn new() -> Self {
        let mut id_to_hash = HashMap::new();
        let mut hash_to_id = HashMap::new();

        // Register native contracts
        let native_mappings = [
            (Self::LEDGER_CONTRACT_ID, LedgerContract::new().hash()),
            (Self::NEO_TOKEN_ID, NeoToken::new().hash()),
            (Self::GAS_TOKEN_ID, GasToken::new().hash()),
            (Self::POLICY_CONTRACT_ID, PolicyContract::new().hash()),
            (Self::ROLE_MANAGEMENT_ID, RoleManagement::new().hash()),
            (Self::ORACLE_CONTRACT_ID, OracleContract::new().hash()),
        ];

        for (id, hash) in native_mappings {
            id_to_hash.insert(id, hash);
            hash_to_id.insert(hash, id);
        }

        Self {
            id_to_hash,
            hash_to_id,
        }
    }

    /// Registers a user contract mapping.
    pub fn register_contract(&mut self, id: i32, hash: UInt160) {
        self.id_to_hash.insert(id, hash);
        self.hash_to_id.insert(hash, id);
    }

    /// Converts raw storage changes to neo-state format.
    ///
    /// # Arguments
    /// * `raw_changes` - Vector of (key_bytes, value_bytes) from DataCache
    ///
    /// # Returns
    /// StateChanges suitable for StateTrieManager
    pub fn convert_to_state_changes(
        &self,
        raw_changes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    ) -> neo_state::StateChanges {
        let mut state_changes = neo_state::StateChanges::new();

        for (key_bytes, value_opt) in raw_changes {
            if key_bytes.len() < 4 {
                continue; // Invalid key format
            }

            // Extract contract ID from first 4 bytes (little-endian)
            let contract_id =
                i32::from_le_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);

            // Get contract hash from ID
            let contract_hash = match self.id_to_hash.get(&contract_id) {
                Some(hash) => *hash,
                None => {
                    // For unknown contracts, use a deterministic hash based on ID
                    Self::id_to_deterministic_hash(contract_id)
                }
            };

            // Extract key suffix (everything after contract ID)
            let key_suffix = key_bytes[4..].to_vec();

            // Create neo-state StorageKey
            let storage_key = StorageKey::new(contract_hash, key_suffix);

            // Convert value
            let storage_item = value_opt.map(StorageItem::new);

            state_changes.storage.insert(storage_key, storage_item);
        }

        state_changes
    }

    /// Converts a single raw key to neo-state StorageKey.
    pub fn convert_key(&self, key_bytes: &[u8]) -> Option<StorageKey> {
        if key_bytes.len() < 4 {
            return None;
        }

        let contract_id =
            i32::from_le_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);

        let contract_hash = self
            .id_to_hash
            .get(&contract_id)
            .copied()
            .unwrap_or_else(|| Self::id_to_deterministic_hash(contract_id));

        let key_suffix = key_bytes[4..].to_vec();
        Some(StorageKey::new(contract_hash, key_suffix))
    }

    /// Gets contract hash from ID.
    pub fn get_hash(&self, id: i32) -> Option<UInt160> {
        self.id_to_hash.get(&id).copied()
    }

    /// Gets contract ID from hash.
    pub fn get_id(&self, hash: &UInt160) -> Option<i32> {
        self.hash_to_id.get(hash).copied()
    }

    /// Creates a deterministic hash from contract ID for unknown contracts.
    fn id_to_deterministic_hash(id: i32) -> UInt160 {
        let mut bytes = [0u8; 20];
        let id_bytes = id.to_le_bytes();
        bytes[0..4].copy_from_slice(&id_bytes);
        // Fill remaining bytes with a pattern based on ID
        for (index, byte) in bytes.iter_mut().enumerate().skip(4) {
            *byte = ((id.wrapping_mul(index as i32)) & 0xFF) as u8;
        }
        UInt160::from(bytes)
    }

    /// Returns the number of registered contracts.
    pub fn contract_count(&self) -> usize {
        self.id_to_hash.len()
    }
}

impl Default for StorageKeyConverter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_converter_creation() {
        let converter = StorageKeyConverter::new();
        // Should have native contracts registered
        assert!(converter.contract_count() >= 6);
    }

    #[test]
    fn test_native_contract_lookup() {
        let converter = StorageKeyConverter::new();

        // Ledger contract
        let ledger_hash = converter.get_hash(StorageKeyConverter::LEDGER_CONTRACT_ID);
        assert!(ledger_hash.is_some());

        // Policy contract
        let policy_hash = converter.get_hash(StorageKeyConverter::POLICY_CONTRACT_ID);
        assert!(policy_hash.is_some());
    }

    #[test]
    fn test_register_user_contract() {
        let mut converter = StorageKeyConverter::new();
        let hash = UInt160::from([0xAA; 20]);
        let id = 100;

        converter.register_contract(id, hash);

        assert_eq!(converter.get_hash(id), Some(hash));
        assert_eq!(converter.get_id(&hash), Some(id));
    }

    #[test]
    fn test_convert_key() {
        let converter = StorageKeyConverter::new();

        // Create a key with Ledger contract ID (-1)
        let mut key_bytes = Vec::new();
        key_bytes.extend_from_slice(&(-1i32).to_le_bytes());
        key_bytes.extend_from_slice(&[0x01, 0x02, 0x03]);

        let storage_key = converter.convert_key(&key_bytes).unwrap();
        assert_eq!(storage_key.key, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_convert_to_state_changes() {
        let converter = StorageKeyConverter::new();

        // Create raw changes
        let mut key1 = (-1i32).to_le_bytes().to_vec();
        key1.extend_from_slice(&[0x01]);

        let mut key2 = (-2i32).to_le_bytes().to_vec();
        key2.extend_from_slice(&[0x02]);

        let raw_changes = vec![
            (key1, Some(vec![0xAA, 0xBB])),
            (key2, None), // Deletion
        ];

        let state_changes = converter.convert_to_state_changes(raw_changes);

        assert_eq!(state_changes.storage.len(), 2);
    }

    #[test]
    fn test_unknown_contract_deterministic_hash() {
        let hash1 = StorageKeyConverter::id_to_deterministic_hash(999);
        let hash2 = StorageKeyConverter::id_to_deterministic_hash(999);
        let hash3 = StorageKeyConverter::id_to_deterministic_hash(1000);

        // Same ID should produce same hash
        assert_eq!(hash1, hash2);
        // Different IDs should produce different hashes
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_invalid_key_handling() {
        let converter = StorageKeyConverter::new();

        // Key too short
        let short_key = vec![0x01, 0x02];
        assert!(converter.convert_key(&short_key).is_none());

        // Empty key
        let empty_key: Vec<u8> = vec![];
        assert!(converter.convert_key(&empty_key).is_none());
    }
}
