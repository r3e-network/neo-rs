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
//! The stubs in this crate return empty / zero values from every
//! storage query. A real executor should wire them up to a
//! populated native-contract cache backed by the
//! `ApplicationEngine`.

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
pub mod provider;
pub mod role_management;
pub mod std_lib;
pub mod treasury;

pub use provider::{install, StandardNativeProvider};
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

/// Reads a native-contract integer setting from `snapshot` under
/// `(contract_id, prefix)`, returning `default` when the key is absent.
///
/// Native settings (fee-per-byte, storage price, oracle price, …) are stored as
/// C# `BigInteger` values in signed little-endian bytes; C# reads them via
/// `(long)(BigInteger)snapshot[key]`. The value is written at contract
/// initialization, so absence only happens pre-genesis / in tests, where the
/// caller supplies the same default the init routine would write.
pub(crate) fn read_storage_int(
    snapshot: &neo_storage::persistence::DataCache,
    contract_id: i32,
    prefix: u8,
    default: i64,
) -> neo_error::CoreResult<i64> {
    use num_traits::ToPrimitive;
    let key = neo_storage::StorageKey::new(contract_id, vec![prefix]);
    match snapshot.get(&key) {
        Some(item) => num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                neo_error::CoreError::invalid_operation("native storage integer out of range")
            }),
        None => Ok(default),
    }
}
