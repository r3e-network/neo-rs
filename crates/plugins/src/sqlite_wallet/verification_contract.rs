//! Verification Contract implementation
//!
//! Provides verification contract functionality for Neo blockchain.

use super::contract::Contract;
use serde::{Deserialize, Serialize};

/// Verification Contract structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationContract {
    /// Base contract
    pub contract: Contract,
    /// Public keys
    pub public_keys: Vec<Vec<u8>>,
    /// Required signature count
    pub required_signature_count: u32,
}

impl VerificationContract {
    /// Create a new verification contract
    pub fn new(
        script: Vec<u8>,
        parameter_list: Vec<u8>,
        public_keys: Vec<Vec<u8>>,
        required_signature_count: u32,
    ) -> Self {
        let contract = Contract::new(script, parameter_list);
        Self {
            contract,
            public_keys,
            required_signature_count,
        }
    }

    /// Get contract
    pub fn get_contract(&self) -> &Contract {
        &self.contract
    }

    /// Get public keys
    pub fn get_public_keys(&self) -> &[Vec<u8>] {
        &self.public_keys
    }

    /// Get required signature count
    pub fn get_required_signature_count(&self) -> u32 {
        self.required_signature_count
    }

    /// Check if contract is valid
    pub fn is_valid(&self) -> bool {
        self.contract.is_valid()
            && !self.public_keys.is_empty()
            && self.required_signature_count > 0
            && self.required_signature_count <= self.public_keys.len() as u32
    }
}
