//! Contract implementation
//!
//! Provides contract functionality for Neo blockchain.

use serde::{Deserialize, Serialize};

/// Contract structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    /// Contract script
    pub script: Vec<u8>,
    /// Parameter list
    pub parameter_list: Vec<u8>,
    /// Contract hash
    pub hash: Vec<u8>,
}

impl Contract {
    /// Create a new contract
    pub fn new(script: Vec<u8>, parameter_list: Vec<u8>) -> Self {
        let hash = Self::calculate_hash(&script);
        Self {
            script,
            parameter_list,
            hash,
        }
    }

    /// Calculate contract hash
    fn calculate_hash(script: &[u8]) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        script.hash(&mut hasher);
        let hash = hasher.finish();
        
        // Convert to 32-byte hash
        let mut result = vec![0u8; 32];
        for (i, byte) in hash.to_le_bytes().iter().enumerate() {
            result[i % 32] ^= byte;
        }
        result
    }

    /// Get contract script
    pub fn get_script(&self) -> &[u8] {
        &self.script
    }

    /// Get parameter list
    pub fn get_parameter_list(&self) -> &[u8] {
        &self.parameter_list
    }

    /// Get contract hash
    pub fn get_hash(&self) -> &[u8] {
        &self.hash
    }

    /// Check if contract is valid
    pub fn is_valid(&self) -> bool {
        !self.script.is_empty() && !self.parameter_list.is_empty()
    }
}
