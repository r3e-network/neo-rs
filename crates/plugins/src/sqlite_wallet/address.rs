//! Address implementation
//!
//! Provides address functionality for Neo blockchain.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Neo address structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Address {
    /// Address bytes
    pub bytes: Vec<u8>,
}

impl Address {
    /// Create a new address from bytes
    pub fn new(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() != 20 {
            return Err("Invalid address length".to_string());
        }
        Ok(Self { bytes })
    }

    /// Create address from string
    pub fn from_string(address_str: String) -> Result<Self, String> {
        // In a real implementation, this would decode Base58
        // For now, we'll create a simple hash-based address
        let hash = Self::simple_hash(&address_str);
        Self::new(hash)
    }

    /// Create address from public key
    pub fn from_public_key(public_key: &[u8]) -> Result<Self, String> {
        // In a real implementation, this would use proper cryptographic hashing
        // For now, we'll create a simple hash-based address
        let hash = Self::simple_hash(public_key);
        Self::new(hash)
    }

    /// Simple hash function (placeholder for real crypto)
    fn simple_hash(data: &[u8]) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        let hash = hasher.finish();

        // Convert to 20-byte address
        let mut result = vec![0u8; 20];
        for (i, byte) in hash.to_le_bytes().iter().enumerate() {
            result[i % 20] ^= byte;
        }
        result
    }

    /// Get address as string
    pub fn to_string(&self) -> String {
        // In a real implementation, this would encode to Base58
        // For now, we'll use hex encoding
        hex::encode(&self.bytes)
    }

    /// Get address bytes
    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Check if address is valid
    pub fn is_valid(&self) -> bool {
        self.bytes.len() == 20
    }

    /// Check if address is zero
    pub fn is_zero(&self) -> bool {
        self.bytes.iter().all(|&b| b == 0)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}
