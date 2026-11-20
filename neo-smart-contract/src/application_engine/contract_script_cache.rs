//! Process-wide cache of parsed contract scripts.
//!
//! Keys are the exact NEF script bytes, not contract hashes or update counters:
//! an upgrade can retain the contract hash while replacing its byte code. Both
//! entry count and retained byte code are bounded because callers can deploy
//! arbitrary contracts. Instruction-table memory is additionally bounded by the
//! NEF script-size limit enforced at deployment.
//!
//! Concurrency design:
//! - Parsed scripts live in a [`DashMap`], so a lookup takes one shard lock
//!   instead of a single process-wide mutex.
//! - Parsing never happens while holding a shared lock. A per-script
//!   single-flight gate serialises concurrent loads of the *same* script, so a
//!   burst of calls into one cold contract parses the byte code exactly once
//!   while unrelated scripts keep parsing in parallel.
//! - Insertion order is kept in a small mutex that is held only to push or pop
//!   one key, preserving the original first-in/first-out eviction order without
//!   serialising lookups or parses.

use crate::error::{CoreError as Error, CoreResult as Result};
use dashmap::DashMap;
use neo_vm::Script;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Maximum number of parsed scripts retained process-wide.
pub const CONTRACT_SCRIPT_CACHE_CAPACITY: usize = 256;
/// Maximum retained NEF byte code, in bytes, across all cached scripts.
pub const CONTRACT_SCRIPT_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Environment variable that disables the process-wide script cache.
const CONTRACT_SCRIPT_CACHE_DISABLE_ENV: &str = "NEO_RS_DISABLE_CONTRACT_SCRIPT_CACHE";

/// Observation-only counters for the parsed script cache.
#[derive(Debug, Default)]
pub struct ContractScriptCacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    coalesced_waits: AtomicU64,
    evictions: AtomicU64,
    parses: AtomicU64,
    parse_nanos: AtomicU64,
}

impl ContractScriptCacheStats {
    #[inline]
    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// A lookup that found the script published by another thread while it
    /// waited on that script's single-flight gate.
    #[inline]
    fn record_coalesced(&self) {
        self.coalesced_waits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_parse(&self, elapsed: Duration) {
        self.parses.fetch_add(1, Ordering::Relaxed);
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.parse_nanos.fetch_add(nanos, Ordering::Relaxed);
    }
}

/// Point-in-time view of the parsed script cache.
///
/// `Default` is the zero snapshot a caller uses when no cache is running, so a
/// missing cache and an idle cache stay distinguishable by the caller's own
/// presence flag rather than by guessed values.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContractScriptCacheStatsSnapshot {
    /// Lookups served without parsing.
    pub hits: u64,
    /// Lookups that had to consult the parse path, including coalesced waits.
    pub misses: u64,
    /// Lookups that waited on another thread's parse and then found it published.
    pub coalesced_waits: u64,
    /// Entries dropped to honour the entry or byte budget.
    pub evictions: u64,
    /// Scripts actually parsed by this process.
    pub parses: u64,
    /// Accumulated parse cost of `parses`.
    pub parse_nanos: u64,
    /// Resident entries at snapshot time.
    pub retained_entries: usize,
    /// Resident NEF byte code at snapshot time.
    pub retained_bytes: usize,
    /// Configured entry bound.
    pub capacity: usize,
    /// Configured byte bound.
    pub max_bytes: usize,
}

impl ContractScriptCacheStatsSnapshot {
    /// Fraction of lookups served from the cache.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Average parse cost across all scripts parsed by this process.
    pub fn average_parse_time(&self) -> Duration {
        self.parse_nanos
            .checked_div(self.parses)
            .map_or(Duration::ZERO, Duration::from_nanos)
    }
}

/// Shared parsed-script cache with bounded entries and byte code.
pub(crate) struct ParsedContractScriptCache {
    scripts: DashMap<Arc<[u8]>, Script>,
    /// First-in/first-out keys. Guarded only for `push_back`/`pop_front`.
    insertion_order: Mutex<VecDeque<Arc<[u8]>>>,
    /// Single-flight gates, one per script currently being parsed.
    in_flight: Mutex<HashMap<Arc<[u8]>, Arc<Mutex<()>>>>,
    retained_bytes: AtomicUsize,
    stats: ContractScriptCacheStats,
    capacity: usize,
    max_bytes: usize,
}

impl ParsedContractScriptCache {
    fn production() -> Self {
        Self::new(
            CONTRACT_SCRIPT_CACHE_CAPACITY,
            CONTRACT_SCRIPT_CACHE_MAX_BYTES,
        )
    }

    fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            scripts: DashMap::new(),
            insertion_order: Mutex::new(VecDeque::new()),
            in_flight: Mutex::new(HashMap::new()),
            retained_bytes: AtomicUsize::new(0),
            stats: ContractScriptCacheStats::default(),
            capacity: capacity.max(1),
            max_bytes,
        }
    }

    /// Returns the parsed script, parsing it at most once even when several
    /// threads request the same cold script concurrently.
    fn get_or_parse(&self, script_bytes: &[u8]) -> Result<Script> {
        self.get_or_parse_with_byte_budget(script_bytes, self.max_bytes)
    }

    fn get_or_parse_with_byte_budget(
        &self,
        script_bytes: &[u8],
        byte_budget: usize,
    ) -> Result<Script> {
        if let Some(script) = self.scripts.get(script_bytes) {
            self.stats.record_hit();
            return Ok(script.clone());
        }

        self.stats.record_miss();

        // An individually oversized script still executes normally. Bypass the
        // cache before eviction so it cannot flush every useful hot contract.
        if script_bytes.len() > byte_budget {
            return self.parse(script_bytes);
        }

        let key: Arc<[u8]> = Arc::from(script_bytes);
        let gate = self.gate_for(Arc::clone(&key));
        let guard = gate.lock();

        // The thread that held the gate before us may have published this
        // script while we waited.
        if let Some(script) = self.scripts.get(script_bytes) {
            let script = script.clone();
            drop(guard);
            self.release_gate(&key, &gate);
            self.stats.record_coalesced();
            return Ok(script);
        }

        let parsed = self.parse(script_bytes);
        if let Ok(script) = &parsed {
            self.publish(Arc::clone(&key), script.clone(), byte_budget);
        }
        drop(guard);
        self.release_gate(&key, &gate);
        parsed
    }

    fn parse(&self, script_bytes: &[u8]) -> Result<Script> {
        let started = Instant::now();
        let parsed = Script::from_shared(Arc::from(script_bytes))
            .map_err(|error| Error::invalid_operation(format!("Invalid script: {error}")));
        self.stats.record_parse(started.elapsed());
        parsed
    }

    fn gate_for(&self, key: Arc<[u8]>) -> Arc<Mutex<()>> {
        let mut in_flight = self.in_flight.lock();
        Arc::clone(in_flight.entry(key).or_default())
    }

    fn release_gate(&self, key: &Arc<[u8]>, gate: &Arc<Mutex<()>>) {
        let mut in_flight = self.in_flight.lock();
        if in_flight
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, gate))
        {
            in_flight.remove(key);
        }
    }

    fn publish(&self, key: Arc<[u8]>, script: Script, byte_budget: usize) {
        self.evict_until_fits(key.len(), byte_budget);
        let previous = self.scripts.insert(Arc::clone(&key), script);
        if previous.is_none() {
            self.retained_bytes.fetch_add(key.len(), Ordering::Relaxed);
            self.insertion_order.lock().push_back(key);
        }
    }

    /// Drops oldest entries until the incoming key fits both bounds.
    ///
    /// The loop consults the same entry and byte limits as the previous
    /// single-lock implementation, but it holds the order lock only for one
    /// `pop_front` at a time, never across a parse.
    fn evict_until_fits(&self, incoming_len: usize, byte_budget: usize) {
        let byte_budget = byte_budget.min(self.max_bytes);
        loop {
            let over_entries = self.scripts.len() >= self.capacity;
            let over_bytes = self
                .retained_bytes
                .load(Ordering::Relaxed)
                .saturating_add(incoming_len)
                > byte_budget;
            if !over_entries && !over_bytes {
                return;
            }
            let Some(oldest) = self.insertion_order.lock().pop_front() else {
                return;
            };
            if self.scripts.remove(&oldest).is_some() {
                let removed = oldest.len();
                let _ = self.retained_bytes.try_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |current| Some(current.saturating_sub(removed)),
                );
                self.stats.record_eviction();
            }
        }
    }

    fn snapshot(&self) -> ContractScriptCacheStatsSnapshot {
        ContractScriptCacheStatsSnapshot {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            coalesced_waits: self.stats.coalesced_waits.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            parses: self.stats.parses.load(Ordering::Relaxed),
            parse_nanos: self.stats.parse_nanos.load(Ordering::Relaxed),
            retained_entries: self.scripts.len(),
            retained_bytes: self.retained_bytes.load(Ordering::Relaxed),
            capacity: self.capacity,
            max_bytes: self.max_bytes,
        }
    }
}

fn cache_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os(CONTRACT_SCRIPT_CACHE_DISABLE_ENV).is_none())
}

/// The single process-wide script cache instance.
static SCRIPT_CACHE: OnceLock<ParsedContractScriptCache> = OnceLock::new();

fn script_cache() -> &'static ParsedContractScriptCache {
    SCRIPT_CACHE.get_or_init(ParsedContractScriptCache::production)
}

/// Returns the parsed script for `script_bytes`, consulting the process-wide
/// cache when it is enabled.
pub(crate) fn cached_contract_script(script_bytes: &[u8]) -> Result<Script> {
    if !cache_enabled() {
        return Script::from_shared(Arc::from(script_bytes))
            .map_err(|error| Error::invalid_operation(format!("Invalid script: {error}")));
    }
    script_cache().get_or_parse(script_bytes)
}

/// Returns a snapshot of the process-wide script cache telemetry.
///
/// `None` means the cache has never run in this process, which callers must
/// distinguish from a running cache that happens to be idle.
pub fn contract_script_cache_stats() -> Option<ContractScriptCacheStatsSnapshot> {
    SCRIPT_CACHE.get().map(ParsedContractScriptCache::snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreResult;
    use neo_vm::OpCode;
    use std::sync::Barrier;
    use std::thread;

    fn script(value: u16) -> Vec<u8> {
        let value = value.to_le_bytes();
        vec![
            OpCode::PUSHINT16.byte(),
            value[0],
            value[1],
            OpCode::RET.byte(),
        ]
    }

    #[test]
    fn content_cache_shares_immutable_bytes_and_instructions() {
        let cache = ParsedContractScriptCache::new(8, 1024);
        let bytes = [OpCode::PUSH1.byte(), OpCode::RET.byte()];

        let first = cache.get_or_parse(&bytes).unwrap();
        let second = cache.get_or_parse(&bytes).unwrap();

        assert_eq!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
        assert!(Arc::ptr_eq(
            &first.get_instruction(0).unwrap(),
            &second.get_instruction(0).unwrap()
        ));
        let snapshot = cache.snapshot();
        assert_eq!(snapshot.hits, 1);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.parses, 1);
    }

    #[test]
    fn content_cache_distinguishes_equal_length_scripts() {
        let cache = ParsedContractScriptCache::new(8, 1024);
        let first = cache
            .get_or_parse(&[OpCode::PUSH1.byte(), OpCode::RET.byte()])
            .unwrap();
        let second = cache
            .get_or_parse(&[OpCode::PUSH2.byte(), OpCode::RET.byte()])
            .unwrap();

        assert_ne!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
        assert_eq!(first.get_instruction(0).unwrap().opcode(), OpCode::PUSH1);
        assert_eq!(second.get_instruction(0).unwrap().opcode(), OpCode::PUSH2);
    }

    #[test]
    fn content_cache_evicts_oldest_script_at_capacity() {
        let cache = ParsedContractScriptCache::new(CONTRACT_SCRIPT_CACHE_CAPACITY, 4096);
        let oldest = script(0);

        for value in 0..=CONTRACT_SCRIPT_CACHE_CAPACITY as u16 {
            cache.get_or_parse(&script(value)).unwrap();
        }

        let snapshot = cache.snapshot();
        assert_eq!(snapshot.retained_entries, CONTRACT_SCRIPT_CACHE_CAPACITY);
        assert_eq!(snapshot.evictions, 1);
        assert!(cache.scripts.get(oldest.as_slice()).is_none());
        assert!(
            cache
                .scripts
                .get(script(CONTRACT_SCRIPT_CACHE_CAPACITY as u16).as_slice())
                .is_some()
        );
        assert_eq!(
            cache.insertion_order.lock().len(),
            snapshot.retained_entries
        );
    }

    #[test]
    fn oversized_script_bypasses_cache_without_evicting_hot_entries() {
        let cache = ParsedContractScriptCache::new(8, 4096);
        let hot_bytes = [OpCode::PUSH1.byte(), OpCode::RET.byte()];
        let hot = cache.get_or_parse(&hot_bytes).unwrap();
        let retained_bytes = cache.snapshot().retained_bytes;

        let oversized = cache
            .get_or_parse_with_byte_budget(&[OpCode::PUSH2.byte(), OpCode::RET.byte()], 1)
            .unwrap();

        assert_eq!(
            oversized.get_instruction(0).unwrap().opcode(),
            OpCode::PUSH2
        );
        assert_eq!(cache.snapshot().retained_entries, 1);
        assert_eq!(cache.snapshot().retained_bytes, retained_bytes);
        let hot_again = cache.get_or_parse(&hot_bytes).unwrap();
        assert_eq!(hot.as_bytes().as_ptr(), hot_again.as_bytes().as_ptr());
    }

    #[test]
    fn concurrent_lookups_of_one_cold_script_parse_it_once() {
        const THREADS: usize = 16;
        let cache = ParsedContractScriptCache::new(16, 4096);
        let barrier = Barrier::new(THREADS);
        let bytes = script(7);

        let pointers: Vec<usize> = thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let barrier = &barrier;
                    let bytes = &bytes;
                    let cache = &cache;
                    scope.spawn(move || -> CoreResult<usize> {
                        barrier.wait();
                        Ok(cache.get_or_parse(bytes)?.as_bytes().as_ptr() as usize)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("thread").expect("script"))
                .collect()
        });

        assert!(
            pointers.windows(2).all(|pair| pair[0] == pair[1]),
            "all threads must share one parsed script allocation"
        );
        let snapshot = cache.snapshot();
        assert_eq!(
            snapshot.parses, 1,
            "thundering herd must parse exactly once"
        );
        assert_eq!(snapshot.retained_entries, 1);
        assert_eq!(snapshot.hits + snapshot.coalesced_waits, THREADS as u64 - 1);
    }

    #[test]
    fn concurrent_distinct_scripts_do_not_share_one_gate() {
        const THREADS: usize = 8;
        let cache = ParsedContractScriptCache::new(64, 1 << 20);
        let barrier = Barrier::new(THREADS);

        thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|index| {
                    let barrier = &barrier;
                    let cache = &cache;
                    scope.spawn(move || {
                        barrier.wait();
                        cache
                            .get_or_parse(&script(index as u16))
                            .expect("distinct script parses")
                            .get_instruction(0)
                            .expect("instruction")
                            .opcode()
                    })
                })
                .collect();
            for handle in handles {
                assert_eq!(handle.join().expect("thread"), OpCode::PUSHINT16);
            }
        });

        let snapshot = cache.snapshot();
        assert_eq!(snapshot.parses, THREADS as u64);
        assert_eq!(snapshot.retained_entries, THREADS);
        assert_eq!(snapshot.evictions, 0);
    }

    #[test]
    fn byte_budget_bounds_retained_scripts() {
        let cache = ParsedContractScriptCache::new(1024, 8);
        for value in 0..3u16 {
            cache.get_or_parse(&script(value)).unwrap();
        }

        let snapshot = cache.snapshot();
        assert!(snapshot.retained_bytes <= snapshot.max_bytes);
        assert!(snapshot.retained_entries >= 1);
        assert!(snapshot.evictions >= 1);
    }
}
