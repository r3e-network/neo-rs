//! Native contract provider — the seam between the abstract engine and the
//! concrete native-contract implementations.
//!
//! The application engine in [`crate::ApplicationEngine`] needs to look
//! up native contracts (NEO, GAS, Policy, ContractManagement, …) by
//! hash, but the engine itself lives in `neo-execution` while the
//! concrete implementations live in `neo-native-contracts`. To avoid
//! the resulting crate-cycle, this module exposes a `NativeContractProvider`
//! trait that:
//!
//! - is **defined** in `neo-execution` (the consumer);
//! - is **implemented** in `neo-native-contracts` (the provider); and
//! - is **registered globally** at process startup, so the engine can
//!   look it up without depending on `neo-native-contracts` directly.
//!
//! The contract:
//!
//! ```ignore
//! // In neo-native-contracts (or a binary):
//! neo_native_contracts::install_provider(Arc::new(MyProvider::new()));
//!
//! // In neo-execution (the engine):
//! let provider = native_contract_provider();
//! if let Some(provider) = provider {
//!     if let Some(contract) = provider.get_native_contract(&hash) {
//!         // ...
//!     }
//! }
//! ```
//!
//! The trait is intentionally narrow — it only exposes the operations
//! the engine needs at runtime (lookup by hash, list of all contracts,
//! current Ledger height, and defaults used for fee/storage prices).

use parking_lot::RwLock;
use std::sync::{Arc, OnceLock};

use neo_primitives::UInt160;

use crate::native_contract::NativeContract;

/// Trait abstracting the lookup of native contracts.
pub trait NativeContractProvider: Send + Sync {
    /// Returns the native contract registered under the given hash.
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>>;

    /// Returns the native contract registered under the given name
    /// (case-insensitive).
    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>>;

    /// Returns all native contracts known to this provider in the
    /// canonical registration order.
    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>>;

    /// Returns all native contract hashes known to this provider.
    fn all_native_contract_hashes(&self) -> Vec<UInt160>;

    /// Returns LedgerContract.CurrentIndex for the supplied snapshot.
    fn current_block_index(&self, snapshot: &neo_storage::DataCache) -> neo_error::CoreResult<u32> {
        let _ = snapshot;
        Ok(0)
    }

    /// Returns the default execution fee factor used when none is
    /// configured (matches `PolicyContract.DEFAULT_EXEC_FEE_FACTOR`).
    fn default_exec_fee_factor(&self) -> u32 {
        30 // Neo N3 default
    }

    /// Returns the default storage price used when none is configured
    /// (matches `PolicyContract.DEFAULT_STORAGE_PRICE`).
    fn default_storage_price(&self) -> u32 {
        100000 // Neo N3 default
    }
}

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn NativeContractProvider>>>> = OnceLock::new();

fn provider_slot() -> &'static RwLock<Option<Arc<dyn NativeContractProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Installs the global native-contract provider. This is normally
/// called once at process startup from `neo-native-contracts::install`
/// (or equivalent). Calling it more than once replaces the previous
/// provider.
pub fn install_provider(provider: Arc<dyn NativeContractProvider>) {
    *provider_slot().write() = Some(provider);
}

/// Returns the currently-installed global native-contract provider, if
/// any. The application engine uses this to dispatch
/// `System.Contract.CallNative` without depending on
/// `neo-native-contracts` directly.
pub fn native_contract_provider() -> Option<Arc<dyn NativeContractProvider>> {
    provider_slot().read().clone()
}

/// Convenience: looks up a native contract by hash via the global
/// provider, returning `None` if no provider is installed or the hash
/// is not registered.
pub fn get_native_contract(hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
    native_contract_provider()?.get_native_contract(hash)
}

/// Convenience: looks up a native contract by name via the global
/// provider.
pub fn get_native_contract_by_name(name: &str) -> Option<Arc<dyn NativeContract>> {
    native_contract_provider()?.get_native_contract_by_name(name)
}

// ============================================================================
// Convenience lookups
// ============================================================================

/// Returns the [`ContractManagement`](neo_native_contracts::ContractManagement)
/// native contract (if installed), looked up by name through the global
/// provider. This is a convenience used by the application engine to
/// reach the one native contract that exposes
/// `lookup_contract_state` semantics for deployed user contracts.
pub fn lookup_contract_management_handle() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("ContractManagement")
}

/// Returns the [`LedgerContract`](neo_native_contracts::LedgerContract)
/// native contract (if installed).
pub fn lookup_ledger_contract() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("LedgerContract")
}

/// Returns the [`PolicyContract`](neo_native_contracts::PolicyContract)
/// native contract (if installed).
pub fn lookup_policy_contract() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("PolicyContract")
}

/// Returns the [`GasToken`](neo_native_contracts::GasToken) native
/// contract (if installed).
pub fn lookup_gas_token() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("GasToken")
}

/// Returns the [`GasToken`](neo_native_contracts::GasToken) contract
/// hash, if installed.
pub fn lookup_gas_token_hash() -> Option<neo_primitives::UInt160> {
    lookup_gas_token().map(|c| c.hash())
}

/// Returns the [`OracleContract`](neo_native_contracts::OracleContract)
/// native contract (if installed).
pub fn lookup_oracle_contract() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("OracleContract")
}

/// Returns the [`NeoToken`](neo_native_contracts::NeoToken) native
/// contract (if installed).
pub fn lookup_neo_token() -> Option<Arc<dyn NativeContract>> {
    get_native_contract_by_name("NeoToken")
}

/// Convenience wrapper around `ContractManagement.lookup_contract_state`.
pub fn lookup_contract_management_state(
    snapshot: &neo_storage::DataCache,
    hash: &neo_primitives::UInt160,
) -> neo_error::CoreResult<Option<crate::ContractState>> {
    let Some(provider) = native_contract_provider() else {
        return Ok(None);
    };
    let Some(cm) = provider.get_native_contract_by_name("ContractManagement") else {
        return Ok(None);
    };
    cm.lookup_contract_state(snapshot, hash)
}

/// Convenience wrapper around `PolicyContract.is_contract_blocked`.
pub fn is_contract_blocked_by_policy(
    snapshot: &neo_storage::DataCache,
    contract_hash: &neo_primitives::UInt160,
) -> neo_error::CoreResult<bool> {
    let Some(provider) = native_contract_provider() else {
        return Ok(false);
    };
    let Some(policy) = provider.get_native_contract_by_name("PolicyContract") else {
        return Ok(false);
    };
    policy.is_contract_blocked(snapshot, contract_hash)
}

/// Convenience wrapper around `NeoToken.committee_address` (C#
/// `NEO.GetCommitteeAddress`). Returns `Ok(None)` when no provider is installed
/// or NeoToken is not registered, so the caller falls back to fail-closed
/// behaviour.
pub fn lookup_committee_address(
    snapshot: &neo_storage::DataCache,
) -> neo_error::CoreResult<Option<neo_primitives::UInt160>> {
    let Some(provider) = native_contract_provider() else {
        return Ok(None);
    };
    let Some(neo) = provider.get_native_contract_by_name("NeoToken") else {
        return Ok(None);
    };
    neo.committee_address(snapshot)
}

/// Convenience wrapper around `PolicyContract.whitelisted_fee`.
pub fn get_whitelisted_fee_for_policy(
    snapshot: &neo_storage::DataCache,
    contract_hash: &neo_primitives::UInt160,
    method: &str,
    param_count: u32,
) -> neo_error::CoreResult<Option<i64>> {
    let Some(provider) = native_contract_provider() else {
        return Ok(None);
    };
    let Some(policy) = provider.get_native_contract_by_name("PolicyContract") else {
        return Ok(None);
    };
    policy.whitelisted_fee(snapshot, contract_hash, method, param_count)
}

/// Convenience: `ContractManagement::get_contract_from_snapshot` from
/// the original `neo-core` code, now routed through the provider.
/// This is the alias used by the application engine.
pub fn lookup_contract_management(
    snapshot: &neo_storage::DataCache,
    hash: &neo_primitives::UInt160,
) -> neo_error::CoreResult<Option<crate::ContractState>> {
    lookup_contract_management_state(snapshot, hash)
}

/// Returns the current block index from the LedgerContract (or 0 if no provider
/// is installed).
pub fn lookup_current_block_index(snapshot: &neo_storage::DataCache) -> u32 {
    let Some(provider) = native_contract_provider() else {
        return 0;
    };
    provider.current_block_index(snapshot).unwrap_or(0)
}
