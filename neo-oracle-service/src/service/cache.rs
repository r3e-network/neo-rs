//! Request-admission cache state for the oracle service.
//!
//! This module owns duplicate-request tracking and finished-request expiry so
//! the service root can stay focused on public service shape and lifecycle
//! wiring.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::{Duration, SystemTime};

use neo_execution::native_contract_provider::NativeContractProvider;

use super::providers::OracleContractReadProvider;
use super::{DEDUP_CACHE_TTL, OracleRuntimeProvider, OracleService, OracleServiceError};

pub(in crate::service) struct ExpiringSet<T> {
    entries: HashMap<T, SystemTime>,
    ttl: Duration,
}

impl<T> ExpiringSet<T>
where
    T: Eq + Hash,
{
    pub(in crate::service) fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    pub(in crate::service) fn insert_at(&mut self, key: T, timestamp: SystemTime) {
        self.entries.insert(key, timestamp);
    }

    pub(in crate::service) fn contains<Q>(&self, key: &Q) -> bool
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

    pub(in crate::service) fn prune_expired(&mut self, now: SystemTime, boundary: ExpiryBoundary) {
        let ttl = self.ttl;
        self.entries.retain(|_, timestamp| {
            now.duration_since(*timestamp)
                .map_or(true, |elapsed| boundary.retains(elapsed, ttl))
        });
    }

    pub(in crate::service) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Copy)]
pub(in crate::service) enum ExpiryBoundary {
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

pub(in crate::service) struct OracleDedupState {
    pub(in crate::service) completed: ExpiringSet<String>,
    pub(in crate::service) in_flight: HashSet<String>,
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

impl<R, P> OracleService<R, P>
where
    R: OracleRuntimeProvider + 'static,
    P: NativeContractProvider + OracleContractReadProvider + 'static,
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
        #[cfg(feature = "oracle")]
        {
            if let Err(reason) = super::super::https::security::Ssrf::validate_url_for_ssrf(url) {
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
