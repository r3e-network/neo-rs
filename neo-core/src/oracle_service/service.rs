//! Oracle service implementation (parity with Neo.Plugins.OracleService).

mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
mod tests;

use crate::cryptography::ECPoint;
use crate::neo_system::NeoSystem;
use crate::network::p2p::payloads::Transaction;
use crate::wallets::Wallet;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::task::JoinHandle;

#[cfg(feature = "oracle")]
pub(super) use super::https::OracleHttpsProtocol;
#[cfg(feature = "oracle")]
pub(super) use super::neofs::OracleNeoFsProtocol;
pub(super) use super::OracleServiceSettings;

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

/// Entry for request deduplication.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct DedupEntry {
    url: String,
    timestamp: SystemTime,
}

/// Oracle service runtime.
pub struct OracleService {
    settings: OracleServiceSettings,
    system: Arc<NeoSystem>,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService>>,
    wallet: RwLock<Option<Arc<dyn Wallet>>>,
    pending_queue: Mutex<HashMap<u64, OracleTask>>,
    finished_cache: Mutex<HashMap<u64, SystemTime>>,
    /// Deduplication cache: URL -> entry with timestamp
    dedup_cache: Mutex<HashMap<String, DedupEntry>>,
    /// In-flight requests to prevent concurrent duplicate processing
    in_flight: Mutex<HashSet<String>>,
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

        let mut dedup_cache = self.dedup_cache.lock();
        let mut in_flight = self.in_flight.lock();
        let now = SystemTime::now();

        // Clean up expired entries
        dedup_cache.retain(|_, entry| {
            if let Ok(elapsed) = now.duration_since(entry.timestamp) {
                elapsed < DEDUP_CACHE_TTL
            } else {
                true
            }
        });

        // Check if URL is currently being processed
        if in_flight.contains(url) {
            tracing::debug!(
                target: "neo::oracle",
                request_id,
                url = %url,
                "Request already in-flight"
            );
            return true;
        }

        // Check if we've seen this URL recently
        if let Some(entry) = dedup_cache.get(url) {
            if let Ok(elapsed) = now.duration_since(entry.timestamp) {
                if elapsed < DEDUP_CACHE_TTL {
                    tracing::debug!(
                        target: "neo::oracle",
                        request_id,
                        url = %url,
                        "Duplicate request detected (recent)"
                    );
                    return true;
                }
            }
        }

        // Mark URL as in-flight
        in_flight.insert(url.to_string());

        false
    }

    /// Marks a request as completed and removes it from in-flight.
    pub fn mark_request_completed(&self, request_id: u64, url: &str) {
        let mut dedup_cache = self.dedup_cache.lock();
        let mut in_flight = self.in_flight.lock();

        in_flight.remove(url);
        dedup_cache.insert(
            url.to_string(),
            DedupEntry {
                url: url.to_string(),
                timestamp: SystemTime::now(),
            },
        );

        tracing::debug!(
            target: "neo::oracle",
            request_id,
            url = %url,
            "Request marked as completed"
        );
    }

    /// Cleans up in-flight requests (call on error/timeout).
    pub fn cleanup_in_flight(&self, url: &str) {
        let mut in_flight = self.in_flight.lock();
        in_flight.remove(url);
    }

    /// Validates a URL against security policies.
    pub fn validate_url(&self, url: &str) -> Result<(), OracleServiceError> {
        // Check whitelist/blacklist
        if !self.settings.is_url_allowed(url) {
            return Err(OracleServiceError::UrlBlocked);
        }

        // Additional SSRF validation (sync version for pre-check)
        #[cfg(feature = "oracle")]
        if let Err(reason) = super::https::security::validate_url_for_ssrf(url) {
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
        self.dedup_cache.lock().len()
    }

    /// Gets the current in-flight request count (for monitoring).
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.lock().len()
    }
}
