//! Oracle service runtime (parity with `Neo.Plugins.OracleService`).
//!
//! This module is the authoritative Oracle service implementation. Its
//! submodule layout mirrors the reference implementation:
//!
//! - [`handlers`]: blockchain-committing and wallet-changed hooks
//! - [`lifecycle`]: construction, start/stop and the unhandled-exception policy
//! - [`processing`]: request/timer loops, URL dispatch and per-request handling
//! - [`transactions`]: pending-queue signature aggregation, threshold relay and
//!   inbound `submitoracleresponse` handling
//! - [`utils`]: signature verification helper
//!
//! Every ledger-, wallet- and signer-facing dependency is expressed through the
//! port traits in [`crate::ports`], so this crate stays free of host-node
//! coupling while preserving the reference behaviour exactly.

mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
mod tests;

pub(crate) use crate::dedup::{ExpiringSet, FINISHED_CACHE_TTL};
pub(crate) use crate::error::OracleServiceError;
pub(crate) use crate::ports::{
    OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder, SignatureVerifier,
    SnapshotFactory, TxRelay,
};
pub(crate) use crate::settings::OracleServiceSettings;

use crate::dedup::OracleDedupState;
use crate::https::OracleHttpsProtocol;
use crate::neofs::OracleNeoFsProtocol;
use crate::security::validate_url_for_ssrf;

use neo_crypto::ECPoint;
use neo_protocol::payloads::Transaction;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use tokio::task::JoinHandle;

/// Refresh interval for the timer loop (3 minutes).
pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_secs(3 * 60);

/// Timeout for sending signatures to other oracle nodes.
pub(crate) const SIGNATURE_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum URL length allowed.
pub(crate) const MAX_URL_LENGTH: usize = 2048;

/// Maximum number of pending requests in the queue.
pub const MAX_PENDING_QUEUE_SIZE: usize = 10000;

/// Maximum time a task can be pending before being considered expired (12 hours).
pub const MAX_TASK_PENDING_TIME: Duration = Duration::from_secs(12 * 60 * 60);

/// Lifecycle state of the oracle service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    /// Created but never started.
    Unstarted,
    /// Currently processing oracle requests.
    Running,
    /// Explicitly stopped after running.
    Stopped,
}

impl OracleStatus {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            OracleStatus::Unstarted => 0,
            OracleStatus::Running => 1,
            OracleStatus::Stopped => 2,
        }
    }

    pub(crate) fn from_u8(value: u8) -> Self {
        match value {
            1 => OracleStatus::Running,
            2 => OracleStatus::Stopped,
            _ => OracleStatus::Unstarted,
        }
    }
}

/// Internal task state for pending oracle responses.
///
/// `signs` and `backup_signs` are keyed by [`ECPoint`] so that their iteration
/// order matches the ordering used by the multi-sig redemption script built in
/// `transactions::check_tx_sign`; `ECPoint`'s `Ord` compares by curve
/// coordinates (as in C#), not by encoded bytes.
pub(crate) struct OracleTask {
    pub(crate) tx: Option<Transaction>,
    pub(crate) backup_tx: Option<Transaction>,
    pub(crate) signs: BTreeMap<ECPoint, Vec<u8>>,
    pub(crate) backup_signs: BTreeMap<ECPoint, Vec<u8>>,
    pub(crate) timestamp: SystemTime,
}

/// Oracle service runtime.
///
/// Generic over the infrastructure traits to allow integration with
/// different ledger/system implementations.
pub struct OracleService<S, W, Sig, TB, R, V>
where
    S: SnapshotFactory + Send + Sync + 'static,
    W: OracleWallet + Send + Sync + 'static,
    Sig: OracleSigner + Send + Sync + 'static,
    TB: ResponseTxBuilder + Send + Sync + 'static,
    R: TxRelay + Send + Sync + 'static,
    V: SignatureVerifier + Send + Sync + 'static,
{
    settings: OracleServiceSettings,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService<S, W, Sig, TB, R, V>>>,
    wallet: RwLock<Option<Arc<W>>>,
    signer: RwLock<Option<Arc<Sig>>>,
    snapshots: Arc<S>,
    tx_builder: Arc<TB>,
    relay: Arc<R>,
    verifier: Arc<V>,
    pending_queue: Mutex<HashMap<u64, OracleTask>>,
    finished_cache: Mutex<ExpiringSet<u64>>,
    dedup: Mutex<OracleDedupState>,
    cancel: AtomicBool,
    request_task: Mutex<Option<JoinHandle<()>>>,
    timer_task: Mutex<Option<JoinHandle<()>>>,
    counter: AtomicU64,
    https: OracleHttpsProtocol,
    neofs: OracleNeoFsProtocol,
}

impl<S, W, Sig, TB, R, V> OracleService<S, W, Sig, TB, R, V>
where
    S: SnapshotFactory + Send + Sync + 'static,
    S::Snapshot: OracleSnapshot,
    W: OracleWallet + Send + Sync + 'static,
    Sig: OracleSigner + Send + Sync + 'static,
    TB: ResponseTxBuilder + Send + Sync + 'static,
    R: TxRelay + Send + Sync + 'static,
    V: SignatureVerifier + Send + Sync + 'static,
{
    /// Checks if a request is a duplicate and should be skipped.
    /// Returns true if the request is a duplicate.
    pub fn is_duplicate_request(&self, request_id: u64, url: &str) -> bool {
        if !self.settings.enable_deduplication {
            return false;
        }

        let now = SystemTime::now();
        let mut dedup = self.dedup.lock();

        // Clean up expired entries
        dedup.prune_expired_completed(now);

        // Check if URL is currently being processed
        if dedup.in_flight.contains(url) {
            tracing::debug!(
                target: "neo::oracle",
                request_id,
                url = %url,
                "Request already in-flight"
            );
            return true;
        }

        // Check if we've seen this URL recently
        if dedup.is_recent_completed(url, now) {
            tracing::debug!(
                target: "neo::oracle",
                request_id,
                url = %url,
                "Duplicate request detected (recent)"
            );
            return true;
        }

        // Mark URL as in-flight
        dedup.start(url);

        false
    }

    /// Marks a request as completed and removes it from in-flight.
    pub fn mark_request_completed(&self, request_id: u64, url: &str) {
        self.dedup.lock().complete(url, SystemTime::now());

        tracing::debug!(
            target: "neo::oracle",
            request_id,
            url = %url,
            "Request marked as completed"
        );
    }

    /// Cleans up in-flight requests (call on error/timeout).
    pub fn cleanup_in_flight(&self, url: &str) {
        self.dedup.lock().cleanup_in_flight(url);
    }

    /// Validates a URL against security policies.
    pub fn validate_url(&self, url: &str) -> Result<(), OracleServiceError> {
        // Check whitelist/blacklist
        if !self.settings.is_url_allowed(url) {
            return Err(OracleServiceError::UrlBlocked);
        }

        // Additional SSRF validation (sync version for pre-check)
        if let Err(reason) = validate_url_for_ssrf(url) {
            tracing::warn!(
                target: "neo::oracle",
                url = %url,
                reason = %reason,
                "URL failed SSRF validation"
            );
            return Err(OracleServiceError::UrlBlocked);
        }

        Ok(())
    }

    /// Gets the current deduplication cache size (for monitoring).
    pub fn dedup_cache_size(&self) -> usize {
        self.dedup.lock().completed.len()
    }

    /// Gets the current in-flight request count (for monitoring).
    pub fn in_flight_count(&self) -> usize {
        self.dedup.lock().in_flight.len()
    }

    /// Signs `payload` through the injected signer port.
    ///
    /// Fails closed when no signer is installed, and never falls back to
    /// another key — matching the reference `sign_neo_payload` behaviour where
    /// the frozen sign intent is resolved against the trusted signer service.
    ///
    /// Public so host-node shells (e.g. neo-core's `OracleService`) can
    /// delegate their signing tests and diagnostics to the engine.
    pub async fn sign_neo_payload(
        &self,
        operation_id: String,
        account: neo_primitives::UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError> {
        let signer = self.signer.read().clone().ok_or_else(|| {
            OracleServiceError::Processing(
                "oracle signing is disabled until a trusted signer service is installed"
                    .to_string(),
            )
        })?;
        signer
            .sign_payload(operation_id, account, payload, protocol, fields)
            .await
    }
}
