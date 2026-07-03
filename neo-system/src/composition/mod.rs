//! # neo-system::composition
//!
//! Composition-root builders, registries, and node assembly helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-system`. This composition crate wires services
//! and must not hide protocol rules or duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `builder`: RPC client builder.
//! - `node`: Daemon composition, CLI modes, and long-running node startup.
//! - `service_registry`: Service registry and lookup helpers.
//! - `wallet_provider`: wallet provider adapter.

pub mod builder;
pub mod node;
pub mod service_registry;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use node::Node;
pub use service_registry::ServiceRegistry;
pub use wallet_provider::WalletProvider;

/// Serializes tests across this module tree that touch the process-global native
/// contract provider (`NativeContractLookup`) — either by building a node (which
/// installs the provider) or by asserting on the installed provider. Without a
/// single shared guard, parallel test threads in the `neo-system` test binary
/// clobber each other's global provider state (a flaky `Arc::ptr_eq` failure).
#[cfg(test)]
pub(crate) static NATIVE_PROVIDER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires [`NATIVE_PROVIDER_TEST_LOCK`], recovering from poisoning so a panic
/// in one guarded test does not cascade `PoisonError` into the others.
#[cfg(test)]
pub(crate) fn native_provider_test_guard() -> std::sync::MutexGuard<'static, ()> {
    NATIVE_PROVIDER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
