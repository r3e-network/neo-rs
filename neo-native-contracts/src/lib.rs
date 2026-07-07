//! # neo-native-contracts
//!
//! Neo N3 native contract implementations and storage codecs.
//!
//! ## Boundary
//!
//! This execution-domain crate owns native contract logic and storage codecs
//! and must not own node startup, RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `registry`: Native contract registry and dispatch helpers.
//! - `support`: Shared support helpers that keep domain modules focused.
//! - `text`: Text segmentation and compatibility helpers for native contracts.
//! - `contract_management`: Native ContractManagement state, storage, and
//!   lifecycle operations.
//! - `crypto_lib`: Native CryptoLib interop surface and verification helpers.
//! - `gas_token`: Native GAS token state, accounting, and transfer behavior.
//! - `ledger_contract`: Native Ledger contract storage and query behavior.
//! - `neo_token`: Native NEO token governance, voting, and committee behavior.
//! - `notary`: Native Notary contract state and request verification behavior.
//! - `oracle_contract`: Native Oracle contract request, response, and fee
//!   behavior.
//! - `policy_contract`: Native Policy contract fee, account, and storage policy
//!   behavior.
//! - `role_management`: Native RoleManagement state and designated-node
//!   behavior.
//! - `std_lib`: Native StdLib string, memory, and serialization helpers.
//! - `test_support`: crate-local test support fixtures.
//! - `treasury`: Native treasury accounting and fund recovery behavior.
//! - `tests`: Module-local tests and regression coverage.

pub use neo_execution::{
    HardforkActivable, NativeContract, NativeContractsCache, NativeContractsCacheEntry,
    NativeEvent, NativeMethod, NativeRegistry, is_active_for,
};

/// Native-contract catalog, hashes, provider, and role definitions.
#[macro_use]
mod macros;
mod nep;
pub mod registry;
mod storage_encoding;
pub(crate) mod support;
mod text;

pub mod contract_management;
pub mod crypto_lib;
pub mod gas_token;
pub mod ledger_contract;
pub mod neo_token;
pub mod notary;
pub mod oracle_contract;
pub mod policy_contract;
pub mod role_management;
pub mod std_lib;
#[cfg(test)]
#[path = "tests/test_support.rs"]
pub(crate) mod test_support;
pub mod treasury;

pub use registry::{catalog, hashes, native_contract, provider, role};
pub(crate) use support::{args, committee, keys};

pub use catalog::{
    STANDARD_NATIVE_CONTRACT_COUNT, StandardNativeContractHashes, StandardNativeContractSpec,
    StandardNativeContractSpecs, is_standard_native_contract_hash, standard_native_contract_hashes,
    standard_native_contract_spec_by_hash, standard_native_contract_spec_by_id,
    standard_native_contract_spec_by_name, standard_native_contract_specs,
    standard_native_contracts,
};
pub use contract_management::ContractManagement;
pub use crypto_lib::CryptoLib;
pub use gas_token::GasToken;
pub use ledger_contract::LedgerContract;
pub use neo_token::NeoToken;
pub use notary::Notary;
pub use oracle_contract::{OracleContract, OracleRequest};
pub use policy_contract::PolicyContract;
pub use provider::{StandardNativeProvider, install};
pub use role::Role;
pub use role_management::RoleManagement;
pub use std_lib::StdLib;
pub use treasury::Treasury;

pub(crate) use nep::{
    AccountState, NEP17_PAYMENT_METHOD, NEP17_STANDARD, NEP17_TRANSFER_EVENT, NEP26_STANDARD,
    NEP27_STANDARD, NEP30_STANDARD, deserialize_account_state, fungible_token_transfer_event,
    native_supported_standards, nep11_payment_method, nep17_account_key, nep17_balance_of_method,
    nep17_decimals_method, nep17_payment_callback_args, nep17_payment_data_item,
    nep17_payment_method, nep17_symbol_method, nep17_total_supply_key, nep17_total_supply_method,
    nep17_transfer_method, nep17_transfer_notification_state, read_nep17_balance,
    read_nep17_total_supply, serialize_account_state,
};
#[cfg(test)]
pub(crate) use nep::{NEP11_PAYMENT_METHOD, NEP17_PREFIX_ACCOUNT, NEP17_PREFIX_TOTAL_SUPPLY};
pub(crate) use storage_encoding::bigint_to_storage_bytes;

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
