//! # Neo Native Contracts
//!
//! Canonical home for the 11 standard Neo native contracts (NEO, GAS,
//! Policy, Oracle, Ledger, ContractManagement, CryptoLib, Notary,
//! RoleManagement, StdLib, Treasury) and the shared
//! `NativeContract` infrastructure.
//!
//! Each native-contract submodule provides a Rust handle type
//! (`NeoToken`, `GasToken`, …) that exposes:
//!
//! - the well-known script hash ([`hashes`])
//! - a stable integer id (`Self::ID`)
//! - the storage-query surface needed by external plugins and
//!   services (`get_request`, `get_designated_by_role_at`, …)
//!
//! The implementations mirror the C# `Neo.SmartContract.Native.*`
//! storage layout (prefix bytes, account-hash encoding, value
//! serialization) so the Rust native-contract surface is
//! byte-compatible with the canonical C# node.

#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(deprecated)]

pub use neo_execution::{
    is_active_for, HardforkActivable, NativeContract, NativeContractsCache,
    NativeContractsCacheEntry, NativeMethod, NativeRegistry,
};

pub mod contract_management;
pub mod crypto_lib;
pub mod gas_token;
pub mod hashes;
pub mod ledger_contract;
pub mod native_contract;
pub mod neo_token;
pub mod notary;
pub mod oracle_contract;
pub mod policy_contract;
pub mod role;
pub mod role_management;
pub mod std_lib;
pub mod treasury;

pub use contract_management::ContractManagement;
pub use crypto_lib::CryptoLib;
pub use gas_token::GasToken;
pub use ledger_contract::LedgerContract;
pub use neo_token::NeoToken;
pub use notary::Notary;
pub use oracle_contract::{OracleContract, OracleRequest};
pub use policy_contract::PolicyContract;
pub use role::Role;
pub use role_management::RoleManagement;
pub use std_lib::StdLib;
pub use treasury::Treasury;

// Helper module
pub mod helpers {
    pub use neo_execution::native_registry::NativeRegistry as NativeHelpers;
}
