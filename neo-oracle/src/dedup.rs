//! Request deduplication for the Oracle service.
//!
//! Tracks in-flight and recently-completed request URLs to avoid
//! redundant fetches within a configurable TTL window.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::{Duration, SystemTime};

/// TTL for request deduplication cache (5 minutes).
pub const DEDUP_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// TTL for finished request cache (3 days).
pub const FINISHED_CACHE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);

/// Boundary semantics for expiry checks.
#[derive(Clone, Copy)]
pub enum ExpiryBoundary {
    /// Entry expires when elapsed > ttl.
    Exclusive,
    /// Entry expires when elapsed >= ttl.
    Inclusive,
}

impl ExpiryBoundary {
    /// Returns true if the entry should be retained given elapsed time and TTL.
    pub fn retains(self, elapsed: Duration, ttl: Duration) -> bool {
        match self {
            Self::Exclusive => elapsed < ttl,
            Self::Inclusive => elapsed <= ttl,
        }
    }
}

/// A set of entries that expire after a TTL.
pub struct ExpiringSet<T> {
    entries: HashMap<T, SystemTime>,
    ttl: Duration,
}

impl<T> ExpiringSet<T>
where
    T: Eq + Hash,
{
    /// Creates a new expiring set with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Inserts an entry with a specific timestamp.
    pub fn insert_at(&mut self, key: T, timestamp: SystemTime) {
        self.entries.insert(key, timestamp);
    }

    /// Checks if the set contains the key (regardless of expiry).
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }

    /// Checks if the set contains a fresh (non-expired) entry for the key.
    pub fn contains_fresh<Q>(&self, key: &Q, now: SystemTime) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key).is_some_and(|timestamp| {
            now.duration_since(*timestamp)
                .is_ok_and(|elapsed| elapsed < self.ttl)
        })
    }

    /// Removes expired entries based on the boundary semantics.
    pub fn prune_expired(&mut self, now: SystemTime, boundary: ExpiryBoundary) {
        let ttl = self.ttl;
        self.entries.retain(|_, timestamp| {
            now.duration_since(*timestamp)
                .map_or(true, |elapsed| boundary.retains(elapsed, ttl))
        });
    }

    /// Returns the number of entries in the set.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Deduplication state tracking in-flight and completed request URLs.
pub struct OracleDedupState {
    /// Recently completed URLs with their completion timestamps.
    pub completed: ExpiringSet<String>,
    /// URLs currently being processed.
    pub in_flight: HashSet<String>,
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
    /// Prunes expired entries from the completed set.
    pub fn prune_expired_completed(&mut self, now: SystemTime) {
        self.completed.prune_expired(now, ExpiryBoundary::Exclusive);
    }

    /// Checks if a URL was recently completed.
    pub fn is_recent_completed(&self, url: &str, now: SystemTime) -> bool {
        self.completed.contains_fresh(url, now)
    }

    /// Marks a URL as in-flight.
    pub fn start(&mut self, url: &str) {
        self.in_flight.insert(url.to_string());
    }

    /// Marks a URL as completed and removes it from in-flight.
    pub fn complete(&mut self, url: &str, timestamp: SystemTime) {
        self.in_flight.remove(url);
        self.completed.insert_at(url.to_string(), timestamp);
    }

    /// Removes a URL from the in-flight set (on error/timeout).
    pub fn cleanup_in_flight(&mut self, url: &str) {
        self.in_flight.remove(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_state_tracks_in_flight_and_completed() {
        let mut state = OracleDedupState::default();
        let url = "https://oracle.example/request";
        let now = SystemTime::now();

        state.start(url);
        assert!(state.in_flight.contains(url));
        assert_eq!(state.completed.len(), 0);

        state.complete(url, now);
        assert!(!state.in_flight.contains(url));
        assert_eq!(state.completed.len(), 1);
        assert!(state.is_recent_completed(url, now));
    }

    #[test]
    fn dedup_state_prunes_expired_entries() {
        let mut state = OracleDedupState::default();
        let url = "https://oracle.example/expired";
        let expired = SystemTime::now() - DEDUP_CACHE_TTL - Duration::from_secs(1);

        state.completed.insert_at(url.to_string(), expired);
        assert_eq!(state.completed.len(), 1);

        state.prune_expired_completed(SystemTime::now());
        assert_eq!(state.completed.len(), 0);
    }

    #[test]
    fn expiring_set_contains_fresh_respects_ttl() {
        let mut set = ExpiringSet::new(Duration::from_secs(60));
        let now = SystemTime::now();

        set.insert_at("key", now);
        assert!(set.contains_fresh("key", now));
        assert!(set.contains_fresh("key", now + Duration::from_secs(30)));
        assert!(!set.contains_fresh("key", now + Duration::from_secs(61)));
    }
}
