//! Native contracts module.
//!
//! This module provides the built-in native contracts for the Neo blockchain,
//! including NEO, GAS, Policy, Notary, and other system contracts.
//!
//! Migrated from `neo-core/src/smart_contract/native/mod.rs`. Import
//! adaptation: `crate::smart_contract::application_engine::*` becomes
//! `neo_smart_contract::application_engine::*`, and `crate::UInt160` becomes
//! `neo_primitives::UInt160`. The `native/` submodule nesting is unchanged, so
//! sibling `super::` references in the concrete contracts keep resolving.

/// Account state for native tokens.
pub mod account_state;
/// Standard native service composition.
pub mod composition;
/// Contract management native contract.
pub mod contract_management;
/// Cryptographic library native contract.
pub mod crypto_lib;
/// Fungible token base implementation.
pub mod fungible_token;
/// GAS token native contract.
pub mod gas_token;
/// Hardfork activation trait for native metadata.
pub mod hardfork_activable;
/// Hash index state for ledger.
pub mod hash_index_state;
/// Helper functions for native contracts.
pub mod helpers;
/// Interoperable list types.
pub mod interoperable_list;
/// Ledger native contract.
pub mod ledger_contract;
pub(crate) mod metadata_macros;
pub(crate) mod method_macros;
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
mod stack_value_numeric;
/// Standard library native contract.
pub mod std_lib;
/// Token management (NFT) contract.
///
/// Not a C# neo v3.10.1 native contract and NOT registered in
/// [`composition::standard_registry`]; kept only as standalone
/// library code. Its `hash()` is not derived from the protocol formula.
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
pub use hardfork_activable::HardforkActivable;
pub use hardfork_activable::HardforkActivable as IHardforkActivable;
pub use helpers::NativeHelpers;
pub use ledger_contract::{LedgerContract, LedgerTransactionStates};
pub use native_contract::{NativeContract, NativeContractsCache, NativeMethod, is_active_for};
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
pub use token_management::{TokenState, TokenType};
pub use transaction_state::TransactionState;
pub use treasury::TreasuryContract;

pub use neo_smart_contract::application_engine::NativeRegistry;

/// Builds a native contract's method table exactly once per process and
/// returns it as a `&'static [NativeMethod]`.
///
/// Native contracts are constructed ad hoc on hot paths (for example
/// `PolicyContract::new().is_blocked_snapshot(..)` on every contract call, or
/// `LedgerContract::new().current_index(..)`). Rebuilding the method table on
/// each construction allocated dozens of `NativeMethod`s (names, parameter
/// vectors, parameter names) per call. The tables are immutable, so sharing a
/// single process-wide instance is behaviour-preserving.
///
/// The builder expression must name the concrete type (not `Self`), because
/// it is evaluated inside a nested `static` item.
macro_rules! static_native_methods {
    ($builder:expr) => {{
        static METHODS: ::std::sync::LazyLock<
            ::std::vec::Vec<neo_smart_contract::application_engine::NativeMethod>,
        > = ::std::sync::LazyLock::new(|| $builder);
        METHODS.as_slice()
    }};
}
pub(crate) use static_native_methods;

/// Parses a well-known native contract hash once per process.
macro_rules! static_contract_hash {
    ($hex:literal) => {{
        static HASH: ::std::sync::LazyLock<neo_primitives::UInt160> =
            ::std::sync::LazyLock::new(|| {
                neo_primitives::UInt160::parse($hex).expect("valid native contract hash literal")
            });
        *HASH
    }};
}
pub(crate) use static_contract_hash;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::{ContractManagement, GasToken, NativeContract, NeoToken};
    use neo_primitives::UInt160;
    use std::sync::Arc;

    #[test]
    fn test_native_registry_creation() {
        let registry = super::composition::standard_registry();

        // Check that standard contracts are registered
        let neo_hash = NeoToken::new().hash();
        let gas_hash = GasToken::new().hash();

        assert!(registry.is_native(&neo_hash));
        assert!(registry.is_native(&gas_hash));
        assert!(registry.get(&neo_hash).is_some());
        assert!(registry.get(&gas_hash).is_some());
    }

    #[test]
    fn stateless_native_contracts_are_shared_but_contract_management_is_per_engine() {
        let first = super::composition::standard_registry();
        let second = super::composition::standard_registry();

        let neo_hash = NeoToken::new().hash();
        assert!(Arc::ptr_eq(
            &first.get(&neo_hash).expect("neo"),
            &second.get(&neo_hash).expect("neo")
        ));

        let management_hash = ContractManagement::new().hash();
        assert!(!Arc::ptr_eq(
            &first.get(&management_hash).expect("management"),
            &second.get(&management_hash).expect("management")
        ));
    }

    #[test]
    fn test_native_registry_all_hashes() {
        let registry = super::composition::standard_registry();
        let hashes = registry.all_hashes();

        // Should have at least NEO and GAS contracts
        assert!(hashes.len() >= 2);
    }

    #[test]
    fn test_native_registry_contract_iteration_order() {
        let registry = super::composition::standard_registry();
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
        ];

        assert_eq!(names, expected);
    }

    #[test]
    fn test_native_registry_hashes_follow_iteration_order() {
        let registry = super::composition::standard_registry();
        let hashes = registry.all_hashes();
        let contract_hashes: Vec<UInt160> = registry
            .contracts()
            .map(|contract| contract.hash())
            .collect();

        assert_eq!(hashes, contract_hashes);
    }

    #[test]
    fn test_native_registry_take_preserves_remaining_order() {
        let mut registry = super::composition::standard_registry();
        let before: Vec<String> = registry
            .contracts()
            .map(|contract| contract.name().to_string())
            .collect();

        let removed = registry
            .take_contract_by_name("GasToken")
            .expect("GasToken should be registered");
        assert_eq!(removed.name(), "GasToken");

        let after: Vec<String> = registry
            .contracts()
            .map(|contract| contract.name().to_string())
            .collect();
        let expected: Vec<String> = before
            .into_iter()
            .filter(|name| name != "GasToken")
            .collect();

        assert_eq!(after, expected);
    }

    #[test]
    fn test_non_native_contract() {
        let registry = super::composition::standard_registry();
        let random_hash = UInt160::zero();

        // Assuming zero hash is not used by native contracts
        if !registry.is_native(&random_hash) {
            assert!(registry.get(&random_hash).is_none());
        }
    }
}
