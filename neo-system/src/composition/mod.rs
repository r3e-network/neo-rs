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
//! - `sync_download_import`: Download-stream to sync-import bridge.
//! - `sync_import_pipeline`: Node-local sync import queue/checkpoint wiring.
//! - `wallet_provider`: wallet provider adapter.
//!
//! `ServiceRegistry` is re-exported from `neo-runtime` — see
//! [`neo_runtime::ServiceRegistry`].

pub mod builder;
mod native_provider;
pub mod node;
pub mod sync_download_import;
pub mod sync_import_pipeline;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use neo_runtime::ServiceRegistry;
pub use node::Node;
pub use sync_download_import::{SyncDownloadImportDriver, SyncDownloadImportSummary};
pub use sync_import_pipeline::SyncImportPipeline;
pub use wallet_provider::WalletProvider;

/// Serializes tests across this module tree that deliberately inspect or reset
/// the process-global native-contract provider (`NativeContractLookup`).
/// Production composition keeps providers local, but these assertions still
/// need one shared guard because the compatibility bridge is a process-global
/// slot inside the `neo-system` test binary.
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
