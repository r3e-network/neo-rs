//! Oracle service implementation (parity with Neo.Plugins.OracleService).

mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
mod tests;

use neo_crypto::ECPoint;
use neo_payloads::Transaction;
use neo_system::Node;
use neo_wallets::Wallet;
use parking_lot::{Mutex, RwLock};
use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    Unstarted,
    Running,
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

#[derive(Debug, Error)]
pub enum OracleServiceError {
    #[error("oracle service disabled")]
    Disabled,
    #[error("oracle request already finished")]
    RequestFinished,
    #[error("oracle request not found")]
    RequestNotFound,
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    #[error("oracle processing error: {0}")]
    Processing(String),
    #[error("duplicate request")]
    DuplicateRequest,
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

struct ExpiringSet<T> {
    entries: HashMap<T, SystemTime>,
    ttl: Duration,
}

impl<T> ExpiringSet<T>
where
    T: Eq + Hash,
{
    fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    fn insert_at(&mut self, key: T, timestamp: SystemTime) {
        self.entries.insert(key, timestamp);
    }

    fn contains<Q>(&self, key: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }

    fn contains_fresh<Q>(&self, key: &Q, now: SystemTime) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key).is_some_and(|timestamp| {
            now.duration_since(*timestamp)
                .is_ok_and(|elapsed| elapsed < self.ttl)
        })
    }

    fn prune_expired(&mut self, now: SystemTime, boundary: ExpiryBoundary) {
        let ttl = self.ttl;
        self.entries.retain(|_, timestamp| {
            now.duration_since(*timestamp)
                .map_or(true, |elapsed| boundary.retains(elapsed, ttl))
        });
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Copy)]
enum ExpiryBoundary {
    Exclusive,
    Inclusive,
}

impl ExpiryBoundary {
    fn retains(self, elapsed: Duration, ttl: Duration) -> bool {
        match self {
            Self::Exclusive => elapsed < ttl,
            Self::Inclusive => elapsed <= ttl,
        }
    }
}

struct OracleDedupState {
    completed: ExpiringSet<String>,
    in_flight: HashSet<String>,
}

impl Default for OracleDedupState {
    fn default() -> Self {
        Self {
            completed: ExpiringSet::new(DEDUP_CACHE_TTL),
            in_flight: HashSet::new(),
        }
    }
}

impl OracleDedupState {
    fn prune_expired_completed(&mut self, now: SystemTime) {
        self.completed.prune_expired(now, ExpiryBoundary::Exclusive);
    }

    fn is_recent_completed(&self, url: &str, now: SystemTime) -> bool {
        self.completed.contains_fresh(url, now)
    }

    fn start(&mut self, url: &str) {
        self.in_flight.insert(url.to_string());
    }

    fn complete(&mut self, url: &str, timestamp: SystemTime) {
        self.in_flight.remove(url);
        self.completed.insert_at(url.to_string(), timestamp);
    }

    fn cleanup_in_flight(&mut self, url: &str) {
        self.in_flight.remove(url);
    }
}

/// Oracle service runtime.
pub struct OracleService {
    settings: OracleServiceSettings,
    system: Arc<Node>,
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

impl OracleService {
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
        #[cfg(feature = "oracle")]
        {
            if let Err(reason) = super::https::security::Ssrf::validate_url_for_ssrf(url) {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %url,
                    reason = %reason,
                    "URL failed SSRF validation"
                );
                return Err(OracleServiceError::UrlBlocked);
            }
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
}
