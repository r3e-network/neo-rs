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
//! - `lifecycle`: Service startup, shutdown, and background processing
//!   lifecycle helpers.
//! - `processing`: oracle request filtering and processing helpers.
//! - `transactions`: oracle response transaction construction helpers.
//! - `utils`: Small utility helpers shared within the crate.
//! - `tests`: Module-local tests and regression coverage.

mod cache;
mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
#[path = "../tests/service/mod.rs"]
mod tests;

use neo_crypto::ECPoint;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Transaction;
use neo_runtime::{ConfigProvider, StoreProvider, TxAdmission};
use neo_wallets::Wallet;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use thiserror::Error;
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

/// Runtime state of the oracle service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    /// Service has been constructed but never started.
    Unstarted,
    /// Service is actively polling and processing oracle requests.
    Running,
    /// Service has been stopped and background tasks have been cancelled.
    Stopped,
}

impl OracleStatus {
    fn as_u8(self) -> u8 {
        match self {
            OracleStatus::Unstarted => 0,
            OracleStatus::Running => 1,
            OracleStatus::Stopped => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => OracleStatus::Running,
            2 => OracleStatus::Stopped,
            _ => OracleStatus::Unstarted,
        }
    }
}

/// Errors returned by oracle service operations.
#[derive(Debug, Error)]
pub enum OracleServiceError {
    /// Oracle service is not running or is disabled by configuration.
    #[error("oracle service disabled")]
    Disabled,
    /// The request was already completed and cached as finished.
    #[error("oracle request already finished")]
    RequestFinished,
    /// The requested oracle entry does not exist in native contract storage.
    #[error("oracle request not found")]
    RequestNotFound,
    /// The supplied public key is not designated as an oracle node.
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    /// The oracle response signature failed verification.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    /// Oracle public key bytes could not be parsed or validated.
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    /// The response transaction for the request could not be located.
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    /// Building the oracle response transaction failed.
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    /// Request processing failed with an implementation-specific reason.
    #[error("oracle processing error: {0}")]
    Processing(String),
    /// The same oracle request is already being processed.
    #[error("duplicate request")]
    DuplicateRequest,
    /// The oracle URL was rejected by the service security policy.
    #[error("URL blocked by security policy")]
    UrlBlocked,
}

struct OracleTask {
    tx: Option<Transaction>,
    backup_tx: Option<Transaction>,
    signs: BTreeMap<ECPoint, Vec<u8>>,
    backup_signs: BTreeMap<ECPoint, Vec<u8>>,
    timestamp: SystemTime,
}

/// Oracle service runtime.
pub struct OracleService {
    settings: OracleServiceSettings,
    config: Arc<dyn ConfigProvider>,
    store: Arc<dyn StoreProvider>,
    tx: Arc<dyn TxAdmission>,
    native_contract_provider: Arc<dyn NativeContractProvider>,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService>>,
    wallet: RwLock<Option<Arc<dyn Wallet>>>,
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
