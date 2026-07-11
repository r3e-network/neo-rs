//! # neo-oracle-service::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `cache`: request deduplication and expiring finished-request cache state.
//! - `handlers`: service message handlers.
//! - `ledger_provider`: ledger read capabilities used by oracle processing.
//! - `lifecycle`: Service startup, shutdown, and background processing
//!   lifecycle helpers.
//! - `native_provider`: native contract read capabilities used by oracle
//!   processing.
//! - `processing`: oracle request filtering and processing helpers.
//! - `status`: service status records.
//! - `task`: pending response-signature task records.
//! - `transactions`: oracle response transaction construction helpers.
//! - `utils`: Small utility helpers shared within the crate.
//! - `tests`: Module-local tests and regression coverage.

mod cache;
mod error;
mod handlers;
mod ledger_provider;
mod lifecycle;
mod native_provider;
mod processing;
mod status;
mod task;
mod transactions;
mod utils;

#[cfg(test)]
#[path = "../tests/service/mod.rs"]
mod tests;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_runtime::{ConfigProvider, StoreProvider, TxAdmission};
use neo_wallets::Nep6Wallet;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::task::JoinHandle;

pub(super) use super::OracleServiceSettings;
#[cfg(feature = "oracle")]
pub(super) use super::https::OracleHttpsProtocol;
#[cfg(feature = "oracle")]
pub(super) use super::neofs::OracleNeoFsProtocol;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3 * 60);
const FINISHED_CACHE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const FILTER_MAX_NEST: usize = 64;
#[cfg(feature = "oracle")]
const SIGNATURE_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// TTL for request deduplication cache (5 minutes).
const DEDUP_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

use cache::{ExpiringSet, OracleDedupState};
pub use error::OracleServiceError;
pub use native_provider::OracleContractReadProvider;
pub use status::OracleStatus;
use task::OracleTask;

/// Combined runtime capabilities required by the oracle service.
///
/// This is a static bound, not an object boundary: service code is still
/// monomorphized over the concrete provider type that the composition root
/// passes in.
pub trait OracleRuntimeProvider: ConfigProvider + StoreProvider + TxAdmission {}

impl<T> OracleRuntimeProvider for T where T: ConfigProvider + StoreProvider + TxAdmission {}

/// Oracle service runtime.
///
/// `R` is the composed runtime provider (usually the node composition root or
/// RPC `NodeContext`) and is kept concrete so oracle request processing does
/// not erase storage/config/transaction-admission calls behind trait objects.
pub struct OracleService<R, P = neo_native_contracts::StandardNativeProvider>
where
    R: OracleRuntimeProvider,
    P: NativeContractProvider + OracleContractReadProvider,
{
    settings: OracleServiceSettings,
    runtime: Arc<R>,
    native_contract_provider: Arc<P>,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService<R, P>>>,
    wallet: RwLock<Option<Arc<Nep6Wallet>>>,
    pending_queue: Mutex<HashMap<u64, OracleTask>>,
    finished_cache: Mutex<ExpiringSet<u64>>,
    /// Deduplication state for completed and in-flight request URLs.
    dedup: Mutex<OracleDedupState>,
    cancel: AtomicBool,
    request_task: Mutex<Option<JoinHandle<()>>>,
    timer_task: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "oracle")]
    counter: AtomicU64,
    #[cfg(feature = "oracle")]
    https: OracleHttpsProtocol,
    #[cfg(feature = "oracle")]
    neofs: OracleNeoFsProtocol,
}
