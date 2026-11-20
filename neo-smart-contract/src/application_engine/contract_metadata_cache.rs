//! Process-wide cache of fully deserialized contract records.
//!
//! Loading a deployed contract from storage deserializes the whole record: the
//! contract id, the update counter, the NEF (including its CRC check over the
//! script) and the manifest. `ApplicationEngine::fetch_contract` performs that
//! work for every `LoadContract`, even though the bytes behind it rarely change.
//!
//! This cache stores the *deserialized* record and is keyed by contract hash
//! plus `update_counter`. ContractManagement increments the counter on every
//! `update`, so an upgrade automatically addresses a different key, while a
//! destroyed-and-redeployed contract is additionally removed by explicit
//! invalidation from the engine's mutation hooks.
//!
//! Only committed state may be published. The engine consults and fills this
//! cache exclusively while it has not yet mutated contract or policy state,
//! mirroring the existing block-wide native contract state cache. That keeps
//! speculative (uncommitted) metadata from a transaction that later faults out
//! of the process-wide map.

use crate::contract_state::ContractState;
use dashmap::DashMap;
use neo_primitives::UInt160;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

/// Maximum number of deserialized contract records retained process-wide.
///
/// Sizing rationale: the standard native contracts plus the typical number of
/// deployed contracts on a public network. Entries are `Arc`-shared and bounded
/// by the NEF script-size limit, so the bound also bounds retained memory.
pub const CONTRACT_METADATA_CACHE_CAPACITY: usize = 512;

/// Environment variable that disables the process-wide contract metadata cache.
const CONTRACT_METADATA_CACHE_DISABLE_ENV: &str = "NEO_RS_DISABLE_CONTRACT_METADATA_CACHE";

/// Cache key: a contract hash is only meaningful together with the update
/// counter that produced its NEF and manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContractMetadataKey {
    /// Contract hash being loaded.
    pub hash: UInt160,
    /// `ContractState::update_counter` of the record the entry was built from.
    pub update_counter: u16,
}

impl ContractMetadataKey {
    /// Builds a key from a contract hash and its record update counter.
    pub fn new(hash: UInt160, update_counter: u16) -> Self {
        Self {
            hash,
            update_counter,
        }
    }
}

/// Observation-only counters for the contract metadata cache.
#[derive(Debug, Default)]
pub struct ContractMetadataCacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    invalidations: AtomicU64,
}

impl ContractMetadataCacheStats {
    #[inline]
    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }
}

/// Point-in-time view of the contract metadata cache.
///
/// `Default` is the zero snapshot a caller uses when no cache is running, so a
/// missing cache and an idle cache stay distinguishable by the caller's own
/// presence flag rather than by guessed values.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContractMetadataCacheStatsSnapshot {
    /// Lookups served from the cache.
    pub hits: u64,
    /// Lookups that had to load and deserialize the contract record.
    pub misses: u64,
    /// Entries dropped to honour the entry bound.
    pub evictions: u64,
    /// Entries removed because the contract was deployed, upgraded or destroyed.
    pub invalidations: u64,
    /// Resident entries at snapshot time.
    pub retained_entries: usize,
    /// Configured entry bound.
    pub capacity: usize,
}

impl ContractMetadataCacheStatsSnapshot {
    /// Fraction of lookups served from the cache.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Bounded concurrent map of deserialized contract records.
pub struct ContractMetadataCache {
    entries: DashMap<ContractMetadataKey, Arc<ContractState>>,
    stats: ContractMetadataCacheStats,
    capacity: usize,
}

impl Default for ContractMetadataCache {
    fn default() -> Self {
        Self::new(CONTRACT_METADATA_CACHE_CAPACITY)
    }
}

impl ContractMetadataCache {
    /// Creates a cache bounded to `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: DashMap::new(),
            stats: ContractMetadataCacheStats::default(),
            capacity: capacity.max(1),
        }
    }

    /// Looks up a record by contract hash and update counter.
    pub fn get(&self, hash: &UInt160, update_counter: u16) -> Option<Arc<ContractState>> {
        let key = ContractMetadataKey::new(*hash, update_counter);
        match self.entries.get(&key) {
            Some(cached) => {
                let contract = Arc::clone(cached.value());
                drop(cached);
                self.stats.record_hit();
                Some(contract)
            }
            None => {
                self.stats.record_miss();
                None
            }
        }
    }

    /// Publishes a record that was read from committed state.
    ///
    /// The key comes from the record itself, so a caller cannot accidentally
    /// stamp an entry with a counter it does not carry.
    pub fn insert(&self, contract: Arc<ContractState>) {
        let key = ContractMetadataKey::new(contract.hash, contract.update_counter);
        if self.entries.contains_key(&key) {
            return;
        }
        if self.entries.len() >= self.capacity {
            self.evict_one(&key);
        }
        // A concurrent insert may win the race; nothing is then retained twice.
        let _ = self.entries.insert(key, contract);
    }

    /// Removes every counter variant of `hash`.
    ///
    /// Called from the engine's deploy/update/destroy hooks so a contract that
    /// is destroyed and later redeployed at the same hash cannot observe the
    /// previous incarnation's metadata.
    pub fn invalidate(&self, hash: &UInt160) {
        let stale: Vec<ContractMetadataKey> = self
            .entries
            .iter()
            .filter(|entry| entry.key().hash == *hash)
            .map(|entry| *entry.key())
            .collect();
        if stale.is_empty() {
            return;
        }
        for key in stale {
            if self.entries.remove(&key).is_some() {
                self.stats.record_invalidation();
            }
        }
    }

    /// Drops every retained record.
    pub fn clear(&self) {
        let retained = self.entries.len();
        self.entries.clear();
        for _ in 0..retained {
            self.stats.record_invalidation();
        }
    }

    /// Number of retained records.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no record is retained.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a snapshot of the cache telemetry.
    pub fn stats_snapshot(&self) -> ContractMetadataCacheStatsSnapshot {
        ContractMetadataCacheStatsSnapshot {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
            retained_entries: self.entries.len(),
            capacity: self.capacity,
        }
    }

    /// Removes one resident entry that is not `incoming`.
    ///
    /// Eviction is arbitrary rather than ordered: the key already partitions
    /// entries by update counter, so old counters are the natural cold set and
    /// an exact order bookkeeping structure is not worth a second lock.
    fn evict_one(&self, incoming: &ContractMetadataKey) {
        let victim = self
            .entries
            .iter()
            .find(|entry| entry.key() != incoming)
            .map(|entry| *entry.key());
        if let Some(victim) = victim
            && self.entries.remove(&victim).is_some()
        {
            self.stats.record_eviction();
        }
    }
}

fn cache_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os(CONTRACT_METADATA_CACHE_DISABLE_ENV).is_none())
}

/// Whether the process-wide metadata cache is enabled for this process.
pub(crate) fn contract_metadata_cache_enabled() -> bool {
    cache_enabled()
}

/// The single process-wide contract metadata cache instance.
static METADATA_CACHE: OnceLock<ContractMetadataCache> = OnceLock::new();

/// Returns the process-wide metadata cache, creating it on first use.
pub(crate) fn contract_metadata_cache() -> &'static ContractMetadataCache {
    METADATA_CACHE.get_or_init(ContractMetadataCache::default)
}

/// Returns the process-wide metadata cache only if it already exists.
///
/// Mutation hooks use this so an engine that never reads a cached record does
/// not allocate the shared cache while invalidating it.
pub(crate) fn initialized_contract_metadata_cache() -> Option<&'static ContractMetadataCache> {
    METADATA_CACHE.get()
}

/// Returns a snapshot of the process-wide metadata cache telemetry.
///
/// `None` means the cache has never run in this process, which callers must
/// distinguish from a running cache that happens to be idle.
pub fn contract_metadata_cache_stats() -> Option<ContractMetadataCacheStatsSnapshot> {
    initialized_contract_metadata_cache().map(ContractMetadataCache::stats_snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_state::NefFile;
    use crate::manifest::ContractManifest;

    fn contract(hash: UInt160, update_counter: u16, name: &str) -> Arc<ContractState> {
        let mut state = ContractState::new(
            update_counter as i32,
            hash,
            NefFile::new("test".to_string(), vec![0x40, 0x41]),
            ContractManifest::new(name.to_string()),
        );
        state.update_counter = update_counter;
        Arc::new(state)
    }

    fn hash(byte: u8) -> UInt160 {
        UInt160::from_bytes(&[byte; 20]).expect("hash")
    }

    #[test]
    fn upgrade_addresses_a_new_key_instead_of_the_stale_record() {
        let cache = ContractMetadataCache::new(8);
        let target = hash(1);
        cache.insert(contract(target, 0, "before"));

        let cached = cache.get(&target, 0).expect("counter 0 is cached");
        assert_eq!(cached.manifest.name, "before");

        // ContractManagement increments update_counter on every upgrade, so the
        // upgraded record is a different key and misses naturally.
        assert!(cache.get(&target, 1).is_none());
        cache.insert(contract(target, 1, "after"));
        assert_eq!(cache.get(&target, 1).unwrap().manifest.name, "after");
        assert_eq!(cache.get(&target, 0).unwrap().manifest.name, "before");

        let snapshot = cache.stats_snapshot();
        assert_eq!(snapshot.hits, 3, "counter 0 twice and counter 1 once");
        assert_eq!(snapshot.misses, 1, "only the upgraded record missed");
    }

    #[test]
    fn explicit_invalidation_drops_every_counter_variant() {
        let cache = ContractMetadataCache::new(8);
        let target = hash(2);
        let other = hash(3);
        cache.insert(contract(target, 0, "v0"));
        cache.insert(contract(target, 1, "v1"));
        cache.insert(contract(other, 0, "other"));

        cache.invalidate(&target);

        assert!(cache.get(&target, 0).is_none());
        assert!(cache.get(&target, 1).is_none());
        assert!(cache.get(&other, 0).is_some());
        assert_eq!(cache.stats_snapshot().invalidations, 2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn entry_bound_never_exceeds_capacity() {
        let cache = ContractMetadataCache::new(4);
        for byte in 0..10u8 {
            cache.insert(contract(hash(byte), 0, "bounded"));
        }

        let snapshot = cache.stats_snapshot();
        assert_eq!(snapshot.retained_entries, 4);
        assert_eq!(cache.len(), 4);
        assert_eq!(snapshot.capacity, 4);
        assert_eq!(snapshot.evictions, 6);
    }

    #[test]
    fn identical_records_are_not_retained_twice() {
        let cache = ContractMetadataCache::new(4);
        let target = hash(4);
        cache.insert(contract(target, 0, "same"));
        cache.insert(contract(target, 0, "same"));

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.stats_snapshot().evictions, 0);
    }

    #[test]
    fn concurrent_lookups_share_one_record_allocation() {
        use std::sync::Barrier;
        use std::thread;

        let cache = ContractMetadataCache::new(16);
        let target = hash(5);
        cache.insert(contract(target, 0, "shared"));
        let barrier = Barrier::new(8);

        thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let barrier = &barrier;
                    let cache = &cache;
                    scope.spawn(move || {
                        barrier.wait();
                        cache.get(&target, 0).expect("shared record")
                    })
                })
                .collect();
            for handle in handles {
                let record = handle.join().expect("thread");
                assert_eq!(record.manifest.name, "shared");
                assert_eq!(record.update_counter, 0);
            }
        });

        assert_eq!(cache.stats_snapshot().hits, 8);
    }

    #[test]
    fn clear_reports_every_dropped_entry() {
        let cache = ContractMetadataCache::new(8);
        cache.insert(contract(hash(6), 0, "a"));
        cache.insert(contract(hash(7), 0, "b"));
        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.stats_snapshot().invalidations, 2);
    }
}
