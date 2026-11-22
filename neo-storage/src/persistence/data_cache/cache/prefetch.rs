use super::*;
impl PersistenceDataCache {
    /// Records an access pattern and returns a bounded scan plan when one is due.
    pub(super) fn record_access_pattern(&self, key: &StorageKey) -> Option<PrefetchScanPlan> {
        let seq = self.access_seq.fetch_add(1, Ordering::Relaxed);
        self.pattern_tracker
            .write()
            .record_access(key, seq, &self.config.prefetch)
    }

    /// Gets the current detected prefetch pattern.
    pub fn current_prefetch_pattern(&self) -> PrefetchPattern {
        self.pattern_tracker
            .read()
            .current_pattern(self.config.prefetch.confidence_threshold)
    }

    /// Checks if a key is inside the prefetch window.
    pub fn is_recently_prefetched(&self, key: &StorageKey) -> bool {
        self.prefetch_window.read().contains(key)
    }

    /// Clears the prefetch window.
    pub fn clear_prefetch_window(&self) {
        self.prefetch_window.write().clear();
    }

    #[inline]
    pub(super) fn prefetch_active(&self) -> bool {
        self.config.prefetch.is_active() && self.read_cache.is_some() && self.store_seek.is_some()
    }

    /// Removes `key` from the window, reporting whether prefetch put it there.
    pub(super) fn take_prefetched(&self, key: &StorageKey) -> bool {
        if !self.config.prefetch.enabled {
            return false;
        }
        self.prefetch_window.write().take(key)
    }

    /// Returns true when this cache owns pending state for `key`.
    ///
    /// Any dictionary entry — a write, a delete, a tombstone or a read mirror —
    /// belongs to the overlay and must never be replaced by a speculative value.
    pub(super) fn has_local_state(&self, key: &StorageKey) -> bool {
        self.state.read().dictionary.contains_key(key)
    }

    /// Runs one bounded speculative scan described by `plan`.
    ///
    /// The scan is confined to `plan.prefix`, reads at most
    /// `limits.max_scan_keys` rows, admits at most `limits.max_admit_keys` keys
    /// and `limits.max_admit_bytes` value bytes, skips anything this cache owns
    /// locally, and tags the batch with the read-cache generation captured
    /// before the scan so a concurrent refresh can reject it.
    pub(super) fn run_prefetch(&self, plan: PrefetchScanPlan) {
        let (Some(cache), Some(seek)) = (self.read_cache.as_ref(), self.store_seek.as_ref()) else {
            return;
        };
        let config = &self.config.prefetch;
        let admit_target = config.admit_target();
        if admit_target == 0 || config.limits.max_scan_keys == 0 {
            self.prefetch_metrics.record_declined();
            return;
        }

        // Snapshot every clock the batch will be validated against before the
        // backing store is touched.
        let source_generation = cache.generation();
        let store_generation = self.observed_generation.load(Ordering::Acquire);

        self.prefetch_metrics.record_request();
        let scanned = seek(
            Some(&plan.prefix),
            Some(&plan.start),
            plan.direction,
            config.limits.max_scan_keys,
        );
        self.prefetch_metrics.record_scan(scanned.len());
        if scanned.is_empty() {
            self.prefetch_metrics.record_declined();
            return;
        }

        // A commit that landed mid-scan makes every row suspect: drop the batch
        // instead of admitting values the refresh has already invalidated.
        if let Some(source) = &self.generation_source
            && source() != store_generation
        {
            self.prefetch_metrics.record_canceled();
            self.clear_prefetch_window();
            return;
        }

        let prefix_bytes = plan.prefix.to_array();
        let mut items: Vec<(StorageKey, StorageItem, usize)> = Vec::with_capacity(admit_target);
        let mut admitted_bytes = 0usize;
        let mut declined = 0u64;
        let mut scanned_kept = 0usize;
        for (key, item) in scanned {
            scanned_kept += 1;
            if items.len() >= admit_target {
                break;
            }
            // Defensive: never admit a row outside the namespace the plan named.
            if !key.as_bytes().starts_with(&prefix_bytes) {
                break;
            }
            if self.is_recently_prefetched(&key) || self.has_local_state(&key) {
                declined += 1;
                continue;
            }
            let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
            if admitted_bytes.saturating_add(size) > config.limits.max_admit_bytes {
                declined += 1;
                break;
            }
            admitted_bytes = admitted_bytes.saturating_add(size);
            items.push((key, item, size));
        }

        if items.is_empty() {
            self.prefetch_metrics.record_declined();
            return;
        }

        {
            let mut window = self.prefetch_window.write();
            for (key, _, _) in &items {
                window.record(key.clone());
            }
        }

        let admission = cache.put_batch_with_generation(items, source_generation);
        self.prefetch_metrics.record_admission(
            admission.admitted as u64,
            admission.stale_skips as u64,
            admission.bytes as u64,
        );
        for _ in 0..declined.saturating_add(admission.rejected as u64) {
            self.prefetch_metrics.record_declined();
        }
        trace!(
            target: "neo",
            direction = ?plan.direction,
            scanned = scanned_kept,
            admitted = admission.admitted,
            "prefetched sequential keys"
        );
    }

    /// Pre-fetches items into the read cache.
    ///
    /// Public entry point for callers that already hold speculative values. The
    /// batch is tagged with the current read-cache generation and never replaces
    /// a resident entry, matching the internal prefetch path.
    pub fn prefetch(&self, items: Vec<(StorageKey, StorageItem)>) {
        if let Some(ref cache) = self.read_cache {
            let generation = cache.generation();
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = v.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    (k, v, size)
                })
                .collect();
            let admission = cache.put_batch_with_generation(cache_items, generation);
            self.prefetch_metrics.record_admission(
                admission.admitted as u64,
                admission.stale_skips as u64,
                admission.bytes as u64,
            );
        }
    }
}
