//! Native contract provider — the seam between the abstract engine and the
//! concrete native-contract implementations.
//!
//! The application engine in [`crate::ApplicationEngine`] needs to look up
//! native contracts (NEO, GAS, Policy, ContractManagement, ...) by hash, but
//! the engine itself lives in `neo-execution` while the concrete
//! implementations live in `neo-native-contracts`. To avoid the resulting
//! crate-cycle, this module exposes a `NativeContractProvider` trait that:
//!
//! - is **defined** in `neo-execution` (the consumer);
//! - is **implemented** in `neo-native-contracts` (the provider); and
//! - is provided by the composition root and captured by new application
//!   engines.
//!
//! The process-global slot is a compatibility bridge for standalone callers
//! and the remaining unconverted helper paths. New execution paths should pass
//! or capture the provider explicitly so one engine cannot observe a provider
//! replacement made by another replay, test, or embedded node.
//!
//! Typical startup and engine construction:
//!
//! ```ignore
//! // In neo-system / neo-node composition:
//! let provider = Arc::new(StandardNativeContractProvider::new(settings));
//! let node = NodeBuilder::new().with_native_contract_provider(provider).build()?;
//!
//! // In tests or replay batches that need a temporary provider:
//! let engine = ApplicationEngine::new_with_native_contract_provider(
//!     trigger,
//!     container,
//!     snapshot,
//!     block,
//!     settings,
//!     gas_limit,
//!     diagnostic,
//!     Some(provider),
//! )?;
//! ```
//!
//! The trait is intentionally narrow — it only exposes the operations
//! the engine needs at runtime (lookup by hash, list of all contracts,
//! current Ledger height, and defaults used for fee/storage prices).

use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

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

thread_local! {
    static SCOPED_PROVIDER: RefCell<Option<Arc<dyn NativeContractProvider>>> =
        RefCell::new(None);
}

fn provider_slot() -> &'static RwLock<Option<Arc<dyn NativeContractProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Process-global serialization lock for tests that mutate the global
/// native-contract provider.
///
/// The provider ([`PROVIDER`]) is a single process-global slot, but tests
/// across `neo-execution` and `neo-blockchain` all install/replace it. Some
/// of those tests share one test binary, so cargo's parallel runner can
/// interleave a provider install from one test with a lookup from another
/// and clobber the installed provider. Every provider-mutating test acquires
/// this one shared lock via [`lock_native_provider`] so they run serially.
static PROVIDER_TEST_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard returned by [`lock_native_provider`]. Holds the shared
/// serialization lock for the duration of a provider-mutating test so that
/// concurrent tests in the same binary cannot interleave a provider install
/// with another test's lookup.
///
/// The guard deliberately does **not** restore a snapshot on drop: each
/// provider-mutating test installs the provider it needs, and the last install
/// under the lock stays until the next locked test installs its own. Restoring
/// to a snapshot on drop would reset the global slot to `None` and clobber a
/// concurrent test that installed a provider without holding this lock.
pub struct NativeProviderTestGuard {
    _lock: MutexGuard<'static, ()>,
}

/// Acquires the process-global native-provider serialization lock. Every test
/// that installs or replaces the global provider must hold this guard for the
/// duration of its body so provider mutations across crates and test binaries
/// stay serialized.
///
/// The lock is poison-tolerant: a panicking test still releases a usable lock
/// to the next test.
pub fn lock_native_provider() -> NativeProviderTestGuard {
    let _lock = PROVIDER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    NativeProviderTestGuard { _lock }
}

/// Process-global native-contract provider compatibility bridge.
///
/// New execution paths should pass an explicit [`NativeContractProvider`].
/// This bridge remains only for standalone callers, tests, and compatibility
/// constructors that need to capture whatever provider the surrounding process
/// installed.
pub struct NativeContractLookup;

impl NativeContractLookup {
    /// Installs the global native-contract provider compatibility bridge.
    ///
    /// Production composition should pass providers explicitly to services and
    /// engines. This helper remains for standalone compatibility callers and
    /// tests that intentionally exercise ambient lookup. Calling it more than
    /// once replaces the previous provider.
    pub fn install_provider(provider: Arc<dyn NativeContractProvider>) {
        Self::replace_provider(Some(provider));
    }

    /// Replaces the global native-contract provider, returning the previous
    /// provider if one was installed.
    pub fn replace_provider(
        provider: Option<Arc<dyn NativeContractProvider>>,
    ) -> Option<Arc<dyn NativeContractProvider>> {
        std::mem::replace(&mut *provider_slot().write(), provider)
    }

    /// Runs `f` while native-contract lookups on the current thread use
    /// `provider` instead of the process-global provider. This lets a bulk
    /// replay batch stay internally consistent without perturbing other
    /// concurrent engine executions.
    pub fn with_scoped_provider<R>(
        provider: Arc<dyn NativeContractProvider>,
        f: impl FnOnce() -> R,
    ) -> R {
        let previous = SCOPED_PROVIDER.with(|slot| slot.replace(Some(provider)));
        struct Reset(Option<Arc<dyn NativeContractProvider>>);
        impl Drop for Reset {
            fn drop(&mut self) {
                let previous = self.0.take();
                SCOPED_PROVIDER.with(|slot| {
                    slot.replace(previous);
                });
            }
        }
        let _reset = Reset(previous);
        f()
    }

    /// Returns the currently-installed or thread-scoped native-contract
    /// provider, if any.
    ///
    /// Compatibility constructors call this before creating an
    /// [`ApplicationEngine`](crate::ApplicationEngine). Once constructed, an
    /// engine uses only the provider it captured and does not observe later
    /// ambient provider changes.
    pub fn native_contract_provider() -> Option<Arc<dyn NativeContractProvider>> {
        SCOPED_PROVIDER
            .with(|slot| slot.borrow().clone())
            .or_else(|| provider_slot().read().clone())
    }
}

#[cfg(test)]
#[path = "../tests/native/native_contract_provider.rs"]
mod tests;
