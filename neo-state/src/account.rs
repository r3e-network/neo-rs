// Copyright (C) 2015-2025 The Neo Project.
//
// account.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! Account state representation for Neo N3.

use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};

/// Represents the state of an account in the Neo blockchain.
///
/// This is a simplified account model that tracks:
/// - Script hash (address)
/// - NEO balance
/// - GAS balance
/// - Vote target (for governance)
/// - Balance height (for GAS calculation)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    /// The script hash (address) of this account.
    pub script_hash: UInt160,

    /// NEO token balance (indivisible, 0 decimals).
    pub neo_balance: u64,

    /// GAS token balance (8 decimals, stored as integer).
    pub gas_balance: u64,

    /// The validator this account is voting for (if any).
    pub vote_to: Option<Vec<u8>>,

    /// Block height when balance was last updated.
    /// Used for GAS distribution calculation.
    pub balance_height: u32,

    /// Last transaction hash that modified this account.
    pub last_updated_tx: Option<UInt256>,
}

impl AccountState {
    /// Creates a new empty account state.
    pub fn new(script_hash: UInt160) -> Self {
        Self {
            script_hash,
            neo_balance: 0,
            gas_balance: 0,
            vote_to: None,
            balance_height: 0,
            last_updated_tx: None,
        }
    }

    /// Creates an account state with initial balances.
    pub fn with_balances(script_hash: UInt160, neo: u64, gas: u64) -> Self {
        Self {
            script_hash,
            neo_balance: neo,
            gas_balance: gas,
            vote_to: None,
            balance_height: 0,
            last_updated_tx: None,
        }
    }

    /// Returns true if this account has no balance and no vote.
    pub fn is_empty(&self) -> bool {
        self.neo_balance == 0 && self.gas_balance == 0 && self.vote_to.is_none()
    }

    /// Returns the NEO balance.
    pub fn neo_balance(&self) -> u64 {
        self.neo_balance
    }

    /// Returns the GAS balance as a fixed-point value (8 decimals).
    pub fn gas_balance(&self) -> u64 {
        self.gas_balance
    }

    /// Returns the GAS balance as a floating-point value.
    pub fn gas_balance_f64(&self) -> f64 {
        self.gas_balance as f64 / 100_000_000.0
    }

    /// Updates the NEO balance.
    pub fn set_neo_balance(&mut self, balance: u64, height: u32) {
        self.neo_balance = balance;
        self.balance_height = height;
    }

    /// Updates the GAS balance.
    pub fn set_gas_balance(&mut self, balance: u64) {
        self.gas_balance = balance;
    }

    /// Adds to the GAS balance.
    pub fn add_gas(&mut self, amount: u64) -> bool {
        match self.gas_balance.checked_add(amount) {
            Some(new_balance) => {
                self.gas_balance = new_balance;
                true
            }
            None => false,
        }
    }

    /// Subtracts from the GAS balance.
    pub fn subtract_gas(&mut self, amount: u64) -> bool {
        match self.gas_balance.checked_sub(amount) {
            Some(new_balance) => {
                self.gas_balance = new_balance;
                true
            }
            None => false,
        }
    }

    /// Sets the vote target.
    pub fn set_vote(&mut self, validator_pubkey: Option<Vec<u8>>) {
        self.vote_to = validator_pubkey;
    }

    /// Returns the vote target public key.
    pub fn vote_to(&self) -> Option<&[u8]> {
        self.vote_to.as_deref()
    }
}

impl Default for AccountState {
    fn default() -> Self {
        Self::new(UInt160::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_state_new() {
        let hash = UInt160::default();
        let account = AccountState::new(hash);

        assert_eq!(account.script_hash, hash);
        assert_eq!(account.neo_balance, 0);
        assert_eq!(account.gas_balance, 0);
        assert!(account.vote_to.is_none());
        assert!(account.is_empty());
    }

    #[test]
    fn test_account_state_with_balances() {
        let hash = UInt160::default();
        let account = AccountState::with_balances(hash, 100, 50_000_000);

        assert_eq!(account.neo_balance(), 100);
        assert_eq!(account.gas_balance(), 50_000_000);
        assert!(!account.is_empty());
    }

    #[test]
    fn test_gas_operations() {
        let mut account = AccountState::new(UInt160::default());

        assert!(account.add_gas(100_000_000));
        assert_eq!(account.gas_balance(), 100_000_000);
        assert_eq!(account.gas_balance_f64(), 1.0);

        assert!(account.subtract_gas(50_000_000));
        assert_eq!(account.gas_balance(), 50_000_000);

        assert!(!account.subtract_gas(100_000_000)); // Would underflow
        assert_eq!(account.gas_balance(), 50_000_000); // Unchanged
    }

    #[test]
    fn test_vote_operations() {
        let mut account = AccountState::new(UInt160::default());

        assert!(account.vote_to().is_none());

        let pubkey = vec![0x02, 0x03, 0x04];
        account.set_vote(Some(pubkey.clone()));

        assert_eq!(account.vote_to(), Some(pubkey.as_slice()));

        account.set_vote(None);
        assert!(account.vote_to().is_none());
    }

    #[test]
    fn test_serialization() {
        let account = AccountState::with_balances(UInt160::default(), 100, 50_000_000);

        let json = serde_json::to_string(&account).unwrap();
        let deserialized: AccountState = serde_json::from_str(&json).unwrap();

        assert_eq!(account, deserialized);
    }
}
