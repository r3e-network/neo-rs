//! Native contracts module.
//!
//! This module provides the built-in native contracts for the Neo blockchain,
//! including NEO, GAS, Policy, Notary, and other system contracts.

/// Account state for native tokens.
pub mod account_state;
/// Contract management native contract.
pub mod contract_management;
/// Cryptographic library native contract.
pub mod crypto_lib;
/// Fungible token base implementation.
pub mod fungible_token;
/// GAS token native contract.
pub mod gas_token;
/// Hash index state for ledger.
pub mod hash_index_state;
/// Helper functions for native contracts.
pub mod helpers;
/// Hardfork activation interface.
pub mod i_hardfork_activable;
/// Ledger native contract.
pub mod ledger_contract;
/// Base native contract implementation.
pub mod native_contract;
/// NEO token native contract.
pub mod neo_token;
/// Notary native contract.
pub mod notary;
/// Oracle native contract.
pub mod oracle_contract;
/// Oracle request types.
pub mod oracle_request;
/// Policy native contract.
pub mod policy_contract;
/// Role definitions for role management.
pub mod role;
/// Role management native contract.
pub mod role_management;
/// Security fixes for native contracts.
pub mod security_fixes;
/// Standard library native contract.
pub mod std_lib;
/// Token management (NFT) native contract.
pub mod token_management;
/// Transaction state for ledger.
pub mod transaction_state;
/// Treasury native contract.
pub mod treasury;
/// Trimmed block representation.
pub mod trimmed_block;

pub use self::oracle_request::OracleRequest;
pub use account_state::AccountState;
pub use contract_management::ContractManagement;
pub use crypto_lib::CryptoLib;
pub use fungible_token::{DefaultTokenAccountState, FungibleToken, TokenAccountState};
pub use gas_token::GasToken;
pub use helpers::NativeHelpers;
pub use i_hardfork_activable::IHardforkActivable;
pub use ledger_contract::{LedgerContract, LedgerTransactionStates};
pub use native_contract::{is_active_for, NativeContract, NativeContractsCache, NativeMethod};
pub use neo_token::NeoToken;
pub use notary::{Deposit as NotaryDeposit, Notary};
pub use oracle_contract::OracleContract;
pub use policy_contract::PolicyContract;
pub use role::Role;
pub use role_management::RoleManagement;
pub use security_fixes::{
    Guard, PermissionValidator, ReentrancyGuardType, SafeArithmetic, SecurityContext,
    StateValidator,
};
pub use std_lib::StdLib;
pub use token_management::{TokenManagement, TokenState, TokenType};
pub use transaction_state::TransactionState;
pub use treasury::TreasuryContract;

use crate::UInt160;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for native contracts.
pub struct NativeRegistry {
    contracts: HashMap<UInt160, Arc<dyn NativeContract>>,
    contract_order: Vec<UInt160>,
}

impl NativeRegistry {
    /// Creates a new native contract registry.
    pub fn new() -> Self {
        let mut registry = Self {
            contracts: HashMap::new(),
            contract_order: Vec::new(),
        };

        // Register standard native contracts
        registry.register_standard_contracts();

        registry
    }

    /// Registers a native contract.
    pub fn register(&mut self, contract: Arc<dyn NativeContract>) {
        let hash = contract.hash();
        if !self.contracts.contains_key(&hash) {
            self.contract_order.push(hash);
        }
        self.contracts.insert(hash, contract);
    }

    /// Gets a native contract by hash.
    pub fn get(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        self.contracts.get(hash).cloned()
    }

    /// Gets a native contract by name.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .values()
            .find(|contract| contract.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    fn find_hash_by_name(&self, name: &str) -> Option<UInt160> {
        self.contracts
            .iter()
            .find(|(_, contract)| contract.name().eq_ignore_ascii_case(name))
            .map(|(hash, _)| *hash)
    }

    pub fn take_contract_by_name(&mut self, name: &str) -> Option<Arc<dyn NativeContract>> {
        let hash = self.find_hash_by_name(name)?;
        self.contract_order.retain(|item| item != &hash);
        self.contracts.remove(&hash)
    }

    /// Checks if a contract hash is a native contract.
    pub fn is_native(&self, hash: &UInt160) -> bool {
        self.contracts.contains_key(hash)
    }

    /// Gets all native contract hashes.
    pub fn all_hashes(&self) -> Vec<UInt160> {
        self.contracts.keys().copied().collect()
    }

    /// Returns all registered native contracts in deterministic registration order.
    ///
    /// Persistence order is consensus-critical. This follows the same declaration order
    /// as neo-project/neo `NativeContract.Contracts`.
    pub fn contracts(&self) -> impl Iterator<Item = Arc<dyn NativeContract>> + '_ {
        self.contract_order
            .iter()
            .filter_map(|hash| self.contracts.get(hash).cloned())
    }

    /// Registers standard Neo native contracts.
    fn register_standard_contracts(&mut self) {
        // Register ContractManagement contract
        self.register(Arc::new(ContractManagement::new()));

        // Register StdLib contract
        self.register(Arc::new(StdLib::new()));

        // Register CryptoLib contract
        self.register(Arc::new(CryptoLib::new()));

        // Register LedgerContract
        self.register(Arc::new(LedgerContract::new()));

        // Register NEO token contract
        self.register(Arc::new(NeoToken::new()));

        // Register GAS token contract
        self.register(Arc::new(GasToken::new()));

        // Register Policy contract
        self.register(Arc::new(PolicyContract::new()));

        // Register RoleManagement contract
        self.register(Arc::new(RoleManagement::new()));

        // Register Oracle contract
        self.register(Arc::new(OracleContract::new()));

        // Register Notary contract (active after HF_Echidna)
        self.register(Arc::new(Notary::new()));

        // Register Treasury contract (active after HF_Faun)
        self.register(Arc::new(TreasuryContract::new()));

        // Register TokenManagement contract (active after HF_Faun)
        self.register(Arc::new(TokenManagement::new()));
    }
}

impl Default for NativeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::{GasToken, NativeContract, NativeRegistry, NeoToken};
    use crate::UInt160;

    #[test]
    fn test_native_registry_creation() {
        let registry = NativeRegistry::new();

        // Check that standard contracts are registered
        let neo_hash = NeoToken::new().hash();
        let gas_hash = GasToken::new().hash();

        assert!(registry.is_native(&neo_hash));
        assert!(registry.is_native(&gas_hash));
        assert!(registry.get(&neo_hash).is_some());
        assert!(registry.get(&gas_hash).is_some());
    }

    #[test]
    fn test_native_registry_all_hashes() {
        let registry = NativeRegistry::new();
        let hashes = registry.all_hashes();

        // Should have at least NEO and GAS contracts
        assert!(hashes.len() >= 2);
    }

    #[test]
    fn test_native_registry_contract_iteration_order() {
        let registry = NativeRegistry::new();
        let names: Vec<String> = registry
            .contracts()
            .map(|contract| contract.name().to_string())
            .collect();

        // Keep this order aligned with neo-project/neo NativeContract.Contracts.
        let expected = vec![
            "ContractManagement".to_string(),
            "StdLib".to_string(),
            "CryptoLib".to_string(),
            "LedgerContract".to_string(),
            "NeoToken".to_string(),
            "GasToken".to_string(),
            "PolicyContract".to_string(),
            "RoleManagement".to_string(),
            "OracleContract".to_string(),
            "Notary".to_string(),
            "Treasury".to_string(),
            "TokenManagement".to_string(),
        ];

        assert_eq!(names, expected);
    }

    #[test]
    fn test_non_native_contract() {
        let registry = NativeRegistry::new();
        let random_hash = UInt160::zero();

        // Assuming zero hash is not used by native contracts
        if !registry.is_native(&random_hash) {
            assert!(registry.get(&random_hash).is_none());
        }
    }
}
