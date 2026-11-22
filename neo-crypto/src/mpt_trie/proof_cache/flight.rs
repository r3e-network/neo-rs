use super::core::ProofCache;
use super::*;
#[derive(Debug, Default)]
pub(super) struct Counters {
    pub(super) hits: AtomicU64,
    pub(super) misses: AtomicU64,
    pub(super) coalesced: AtomicU64,
    pub(super) admissions: AtomicU64,
    pub(super) oversized: AtomicU64,
    pub(super) evictions: AtomicU64,
    pub(super) constructions: AtomicU64,
    pub(super) construction_nanos: AtomicU64,
}

impl Counters {
    pub(super) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn add(counter: &AtomicU64, value: u64) {
        counter.fetch_add(value, Ordering::Relaxed);
    }
}

pub(super) struct CacheEntry {
    bundle: Arc<ProofBundle>,
    bytes: usize,
    tick: u64,
}

/// Outcome published by the thread that performed a traversal.
#[derive(Clone)]
pub(super) enum FlightOutcome {
    Ready(Arc<ProofBundle>),
    Missing,
}

enum FlightState {
    Pending,
    Settled(FlightOutcome),
    Failed,
}

/// Single-flight slot that lets identical concurrent requests share one build.
pub(super) struct Flight {
    state: Mutex<FlightState>,
    settled: Condvar,
}

impl Flight {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(FlightState::Pending),
            settled: Condvar::new(),
        }
    }

    /// Blocks until the leader publishes an outcome.
    ///
    /// Returns `None` when the leader failed, in which case the waiter retries
    /// so a transient error cannot be served as a cached answer.
    pub(super) fn wait(&self) -> Option<FlightOutcome> {
        let mut state = self.state.lock();
        loop {
            match &*state {
                FlightState::Pending => self.settled.wait(&mut state),
                FlightState::Settled(outcome) => return Some(outcome.clone()),
                FlightState::Failed => return None,
            }
        }
    }

    pub(super) fn settle(&self, outcome: FlightOutcome) {
        *self.state.lock() = FlightState::Settled(outcome);
        self.settled.notify_all();
    }

    pub(super) fn fail(&self) {
        *self.state.lock() = FlightState::Failed;
        self.settled.notify_all();
    }
}

/// Releases a leader's flight even when the traversal panics, so waiters are
/// woken and retry instead of blocking forever on a flight nobody will settle.
pub(super) struct FlightGuard<'a> {
    cache: &'a ProofCache,
    key: &'a ProofCacheKey,
    armed: bool,
}

impl<'a> FlightGuard<'a> {
    pub(super) fn new(cache: &'a ProofCache, key: &'a ProofCacheKey) -> Self {
        Self {
            cache,
            key,
            armed: true,
        }
    }

    /// Marks the flight as settled by the caller.
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.cache.fail_flight(self.key);
        }
    }
}

pub(super) struct Inner {
    pub(super) entries: HashMap<ProofCacheKey, CacheEntry>,
    /// `(tick, key)` recency log; stale ticks are skipped during eviction.
    pub(super) recency: VecDeque<(u64, ProofCacheKey)>,
    pub(super) flights: HashMap<ProofCacheKey, Arc<Flight>>,
    pub(super) bytes: usize,
    tick: u64,
}

impl Inner {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            recency: VecDeque::new(),
            flights: HashMap::new(),
            bytes: 0,
            tick: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// Returns the cached bundle and refreshes the entry's recency.
    pub(super) fn lookup(&mut self, key: &ProofCacheKey) -> Option<Arc<ProofBundle>> {
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        entry.tick = tick;
        let bundle = Arc::clone(&entry.bundle);
        self.recency.push_back((tick, key.clone()));
        self.compact_recency();
        Some(bundle)
    }

    /// Inserts one entry and evicts until both bounds hold again.
    pub(super) fn admit(
        &mut self,
        key: ProofCacheKey,
        bundle: Arc<ProofBundle>,
        entry_bytes: usize,
    ) {
        let tick = self.next_tick();
        if let Some(previous) = self.entries.insert(
            key.clone(),
            CacheEntry {
                bundle,
                bytes: entry_bytes,
                tick,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(previous.bytes);
        }
        self.bytes += entry_bytes;
        self.recency.push_back((tick, key));
    }

    /// Evicts least-recently-used entries until both bounds are satisfied.
    ///
    /// The freshly admitted entry is the most recent one, so it is the last
    /// candidate; because an admitted entry always fits the byte bound by its
    /// own, the loop cannot evict it.
    pub(super) fn enforce_bounds(&mut self, config: &ProofCacheConfig) -> u64 {
        let mut evicted = 0;
        while self.bytes > config.max_bytes.get() || self.entries.len() > config.max_entries.get() {
            let Some((tick, key)) = self.recency.pop_front() else {
                break;
            };
            let is_current = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.tick == tick);
            if !is_current {
                continue;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                evicted += 1;
            }
        }
        evicted
    }

    /// Keeps the recency log proportional to the retained entries.
    fn compact_recency(&mut self) {
        let limit = self.entries.len().saturating_mul(4).saturating_add(8);
        if self.recency.len() <= limit {
            return;
        }
        let mut live: Vec<(u64, ProofCacheKey)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.tick, key.clone()))
            .collect();
        live.sort_unstable_by_key(|(tick, _)| *tick);
        self.recency = live.into();
    }
}
