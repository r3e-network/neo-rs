use super::flight::{Counters, Flight, FlightGuard, FlightOutcome, Inner};
use super::*;
/// Byte- and entry-bounded LRU cache of constructed MPT proofs.
///
/// The cache is safe to share between threads; identical concurrent requests
/// are served by a single traversal.
pub struct ProofCache {
    config: ProofCacheConfig,
    inner: Mutex<Inner>,
    counters: Counters,
}

impl ProofCache {
    /// Creates an empty cache with the supplied bounds.
    #[must_use]
    pub fn new(config: ProofCacheConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner::new()),
            counters: Counters::default(),
        }
    }

    /// Creates an empty cache with [`ProofCacheConfig::default`].
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(ProofCacheConfig::default())
    }

    /// Returns the configured bounds.
    #[must_use]
    pub const fn config(&self) -> &ProofCacheConfig {
        &self.config
    }

    /// Returns the cached bundle for `key`, counting a hit or a miss.
    #[must_use]
    pub fn get(&self, key: &ProofCacheKey) -> Option<Arc<ProofBundle>> {
        let mut inner = self.inner.lock();
        match inner.lookup(key) {
            Some(bundle) => {
                Counters::bump(&self.counters.hits);
                Some(bundle)
            }
            None => {
                Counters::bump(&self.counters.misses);
                None
            }
        }
    }

    /// Returns `true` when `key` is retained without changing its recency.
    #[must_use]
    pub fn contains(&self, key: &ProofCacheKey) -> bool {
        self.inner.lock().entries.contains_key(key)
    }

    /// Returns the number of retained proofs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    /// Returns `true` when no proof is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the approximate bytes retained by all entries.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.inner.lock().bytes
    }

    /// Drops every retained entry. In-flight builds are unaffected.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.entries.clear();
        inner.recency.clear();
        inner.bytes = 0;
    }

    /// Returns a point-in-time snapshot of counters and gauges.
    #[must_use]
    pub fn stats(&self) -> ProofCacheStats {
        let (entries, bytes) = {
            let inner = self.inner.lock();
            (inner.entries.len(), inner.bytes)
        };
        ProofCacheStats {
            hits: self.counters.hits.load(Ordering::Relaxed),
            misses: self.counters.misses.load(Ordering::Relaxed),
            coalesced: self.counters.coalesced.load(Ordering::Relaxed),
            admissions: self.counters.admissions.load(Ordering::Relaxed),
            oversized_rejects: self.counters.oversized.load(Ordering::Relaxed),
            evictions: self.counters.evictions.load(Ordering::Relaxed),
            constructions: self.counters.constructions.load(Ordering::Relaxed),
            entries,
            bytes,
            construction_time: Duration::from_nanos(
                self.counters.construction_nanos.load(Ordering::Relaxed),
            ),
        }
    }

    /// Returns a cached bundle or builds it exactly once.
    ///
    /// `build` returns `Ok(Some(nodes))` for a proof, `Ok(None)` when the key is
    /// absent from the trie, and `Err` for a traversal failure. Absent keys and
    /// errors are never cached: the negative outcome stays a property of the
    /// trie, and a later call re-reads it.
    ///
    /// The closure only runs on the thread that wins the traversal, so it does
    /// not need to be `Send`; concurrent callers block on the winner's result.
    pub fn get_or_construct<F>(
        &self,
        key: &ProofCacheKey,
        build: F,
    ) -> MptResult<Option<Arc<ProofBundle>>>
    where
        F: FnOnce() -> MptResult<Option<Vec<Vec<u8>>>>,
    {
        let mut build = Some(build);
        loop {
            let flight = {
                let mut inner = self.inner.lock();
                if let Some(bundle) = inner.lookup(key) {
                    Counters::bump(&self.counters.hits);
                    return Ok(Some(bundle));
                }
                Counters::bump(&self.counters.misses);
                match inner.flights.get(key) {
                    Some(existing) => {
                        Counters::bump(&self.counters.coalesced);
                        Some(Arc::clone(existing))
                    }
                    None => {
                        let created = Arc::new(Flight::new());
                        inner.flights.insert(key.clone(), Arc::clone(&created));
                        // `None` marks this caller as the leader.
                        None
                    }
                }
            };

            if let Some(flight) = flight {
                match flight.wait() {
                    Some(FlightOutcome::Ready(bundle)) => return Ok(Some(bundle)),
                    Some(FlightOutcome::Missing) => return Ok(None),
                    // The leader failed; retry as a fresh request so the error
                    // is not shared as a cached answer.
                    None => continue,
                }
            }

            let builder = build
                .take()
                .expect("the leader runs the build closure at most once");
            let mut guard = FlightGuard::new(self, key);
            let started = Instant::now();
            let outcome = builder();
            let elapsed = started.elapsed();
            Counters::bump(&self.counters.constructions);
            Counters::add(
                &self.counters.construction_nanos,
                u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
            );

            match outcome {
                Ok(Some(nodes)) => {
                    let bundle = Arc::new(ProofBundle::from_nodes(nodes));
                    self.publish(key, &bundle);
                    guard.disarm();
                    return Ok(Some(bundle));
                }
                Ok(None) => {
                    self.finish_flight(key, None);
                    guard.disarm();
                    return Ok(None);
                }
                Err(error) => {
                    self.fail_flight(key);
                    guard.disarm();
                    return Err(error);
                }
            }
        }
    }

    /// Admits a built bundle, then releases waiters.
    ///
    /// Admission happens before the flight is settled so that a request
    /// arriving in between observes the cache entry rather than starting a
    /// duplicate traversal.
    fn publish(&self, key: &ProofCacheKey, bundle: &Arc<ProofBundle>) {
        let entry_bytes = bundle.retained_bytes() + key.retained_bytes();
        let admitted = entry_bytes <= self.config.max_entry_bytes.get();

        if admitted {
            let mut inner = self.inner.lock();
            inner.admit(key.clone(), Arc::clone(bundle), entry_bytes);
            let evicted = inner.enforce_bounds(&self.config);
            drop(inner);
            Counters::bump(&self.counters.admissions);
            Counters::add(&self.counters.evictions, evicted);
        } else {
            // Rejecting an oversized proof must not touch the working set.
            Counters::bump(&self.counters.oversized);
        }

        self.finish_flight(key, Some(FlightOutcome::Ready(Arc::clone(bundle))));
    }

    pub(super) fn fail_flight(&self, key: &ProofCacheKey) {
        let flight = self.inner.lock().flights.remove(key);
        if let Some(flight) = flight {
            flight.fail();
        }
    }

    fn finish_flight(&self, key: &ProofCacheKey, outcome: Option<FlightOutcome>) {
        let flight = self.inner.lock().flights.remove(key);
        let Some(flight) = flight else {
            return;
        };
        match outcome {
            Some(outcome) => flight.settle(outcome),
            None => flight.settle(FlightOutcome::Missing),
        }
    }
}

impl std::fmt::Debug for ProofCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("ProofCache")
            .field("config", &self.config)
            .field("entries", &stats.entries)
            .field("bytes", &stats.bytes)
            .finish()
    }
}
