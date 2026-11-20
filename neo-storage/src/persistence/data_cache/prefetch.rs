//! Speculative storage prefetch: opt-in configuration, pattern detection,
//! bounded windows and counters.
//!
//! Prefetching is a *speculative* read optimisation. Everything here is written
//! so that (a) the feature is inert unless a caller explicitly enables it, and
//! (b) an enabled scan can never read, admit or retain more than the configured
//! ceilings.

use crate::persistence::seek_direction::SeekDirection;
use crate::types::StorageKey;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// Prefetch pattern detection for sequential access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchPattern {
    /// No prefetching.
    None,
    /// Sequential forward access (ascending keys).
    SequentialForward,
    /// Sequential backward access (descending keys).
    SequentialBackward,
    /// Strided access (fixed offset between keys).
    Strided,
}

impl PrefetchPattern {
    /// Returns the scan direction this pattern implies, if any.
    #[must_use]
    pub(super) const fn direction(self) -> Option<SeekDirection> {
        match self {
            Self::SequentialForward => Some(SeekDirection::Forward),
            Self::SequentialBackward => Some(SeekDirection::Backward),
            Self::None | Self::Strided => None,
        }
    }
}

/// Hard ceilings applied to a single prefetch scan.
///
/// The limits are enforced at the source: the backing store is asked for at
/// most [`Self::max_scan_keys`] rows and the read cache receives at most
/// [`Self::max_admit_keys`] entries occupying at most [`Self::max_admit_bytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrefetchLimits {
    /// Maximum number of rows a single scan may read from the backing store.
    pub max_scan_keys: usize,
    /// Maximum number of keys a single scan may admit into the read cache.
    pub max_admit_keys: usize,
    /// Maximum number of value bytes a single scan may admit.
    pub max_admit_bytes: usize,
}

impl PrefetchLimits {
    /// Hard ceiling on rows read from the backing store by a single scan.
    ///
    /// Configuration may lower this but never raise it, so no configuration
    /// source can turn a speculative scan into an unbounded range read.
    pub const CEILING_SCAN_KEYS: usize = 4096;
    /// Hard ceiling on value bytes admitted by a single scan.
    pub const CEILING_ADMIT_BYTES: usize = 4 * 1024 * 1024;
    /// Hard ceiling on keys admitted by a single scan.
    pub const CEILING_ADMIT_KEYS: usize = 1024;

    /// Clamps every ceiling into the hard bounds declared above.
    #[must_use]
    pub const fn sanitized(self) -> Self {
        Self {
            max_scan_keys: clamp_usize(self.max_scan_keys, Self::CEILING_SCAN_KEYS),
            max_admit_keys: clamp_usize(self.max_admit_keys, Self::CEILING_ADMIT_KEYS),
            max_admit_bytes: clamp_usize(self.max_admit_bytes, Self::CEILING_ADMIT_BYTES),
        }
    }
}

/// Returns `value` clamped into `0..=ceiling` without `Ord` bounds (const-callable).
const fn clamp_usize(value: usize, ceiling: usize) -> usize {
    if value > ceiling { ceiling } else { value }
}

impl Default for PrefetchLimits {
    fn default() -> Self {
        Self {
            max_scan_keys: 64,
            max_admit_keys: 8,
            max_admit_bytes: 64 * 1024,
        }
    }
}

/// Opt-in configuration for speculative storage prefetching.
///
/// Disabled by default: a node that does not set `enabled = true` performs the
/// same reads, in the same order, as a build without this feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrefetchConfig {
    /// Whether speculative prefetching runs at all.
    pub enabled: bool,
    /// Number of keys one scan aims to prefetch.
    pub count: usize,
    /// Minimum access-pattern confidence (0-100) before a scan may start.
    pub confidence_threshold: u8,
    /// Minimum number of consecutive sequential observations before a scan.
    pub min_observations: u32,
    /// Count and byte ceilings enforced per scan.
    pub limits: PrefetchLimits,
}

impl PrefetchConfig {
    /// Configuration with prefetching switched off (the production default).
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            count: 0,
            confidence_threshold: 100,
            min_observations: u32::MAX,
            limits: PrefetchLimits {
                max_scan_keys: 0,
                max_admit_keys: 0,
                max_admit_bytes: 0,
            },
        }
    }

    /// Returns true when a scan would be allowed to read and admit work.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.enabled
            && self.count > 0
            && self.limits.max_scan_keys > 0
            && self.limits.max_admit_keys > 0
            && self.limits.max_admit_bytes > 0
    }

    /// Returns a copy whose limits sit inside the hard ceilings.
    ///
    /// Every consumer stores the sanitized form, so a configuration that asks
    /// for an enormous scan can still never materialise one.
    #[must_use]
    pub const fn sanitized(self) -> Self {
        Self {
            limits: self.limits.sanitized(),
            ..self
        }
    }

    /// Number of keys one scan admits at most.
    #[must_use]
    pub(super) fn admit_target(&self) -> usize {
        self.count.min(self.limits.max_admit_keys)
    }
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            count: 8,
            confidence_threshold: 60,
            min_observations: 4,
            limits: PrefetchLimits::default(),
        }
    }
}

/// Counters describing what speculative prefetching actually did.
#[derive(Debug, Default)]
pub(crate) struct PrefetchMetrics {
    requests: AtomicU64,
    keys_scanned: AtomicU64,
    admitted: AtomicU64,
    useful_hits: AtomicU64,
    stale_skips: AtomicU64,
    canceled: AtomicU64,
    declined: AtomicU64,
    bytes: AtomicU64,
}

impl PrefetchMetrics {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_scan(&self, keys: usize) {
        self.keys_scanned.fetch_add(keys as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_useful_hit(&self) {
        self.useful_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_canceled(&self) {
        self.canceled.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_declined(&self) {
        self.declined.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_admission(&self, admitted: u64, stale_skips: u64, bytes: u64) {
        self.admitted.fetch_add(admitted, Ordering::Relaxed);
        self.stale_skips.fetch_add(stale_skips, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> PrefetchMetricsSnapshot {
        PrefetchMetricsSnapshot {
            requests: self.requests.load(Ordering::Relaxed),
            keys_scanned: self.keys_scanned.load(Ordering::Relaxed),
            admitted: self.admitted.load(Ordering::Relaxed),
            useful_hits: self.useful_hits.load(Ordering::Relaxed),
            stale_skips: self.stale_skips.load(Ordering::Relaxed),
            canceled: self.canceled.load(Ordering::Relaxed),
            declined: self.declined.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time view of the prefetch counters.
///
/// `Default` is the zero snapshot, so a caller that finds no cache can report
/// "prefetch never ran" without guessing values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrefetchMetricsSnapshot {
    /// Scans that reached the backing store.
    pub requests: u64,
    /// Rows the backing store handed back across those scans.
    pub keys_scanned: u64,
    /// Keys that entered the read cache.
    pub admitted: u64,
    /// Accesses served from a key this cache prefetched.
    pub useful_hits: u64,
    /// Prefetched keys dropped because a fresher entry was resident.
    pub stale_skips: u64,
    /// Scans abandoned because the backing state moved underneath them.
    pub canceled: u64,
    /// Scans declined before or instead of reading the backing store.
    pub declined: u64,
    /// Bytes admitted into the read cache by prefetching.
    pub bytes: u64,
}

/// Bounded record of the keys the most recent scans already covered.
///
/// A bounded queue is used instead of an unbounded set so speculative state
/// cannot grow with the working set; the oldest covered key is forgotten first.
pub(super) struct PrefetchWindow {
    keys: VecDeque<StorageKey>,
    capacity: usize,
}

impl PrefetchWindow {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            keys: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub(super) fn contains(&self, key: &StorageKey) -> bool {
        !self.keys.is_empty() && self.keys.contains(key)
    }

    pub(super) fn record(&mut self, key: StorageKey) {
        if self.contains(&key) {
            return;
        }
        while self.keys.len() >= self.capacity {
            self.keys.pop_front();
        }
        self.keys.push_back(key);
    }

    /// Removes `key` and reports whether the window held it.
    ///
    /// Consuming the record keeps "recently prefetched" honest: a key is only
    /// recorded until a read actually scores it as a useful hit.
    pub(super) fn take(&mut self, key: &StorageKey) -> bool {
        let Some(index) = self.keys.iter().position(|candidate| candidate == key) else {
            return false;
        };
        self.keys.remove(index);
        true
    }

    pub(super) fn clear(&mut self) {
        self.keys.clear();
    }

    #[allow(dead_code)]
    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }
}

/// A single prefetch scan derived from the detected access pattern.
///
/// `prefix` is the namespace the scan is confined to and `start` is the cursor
/// the scan continues from; the two are deliberately different values so a scan
/// never degenerates into "look for keys that extend the current key".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PrefetchScanPlan {
    pub(super) direction: SeekDirection,
    pub(super) prefix: StorageKey,
    pub(super) start: StorageKey,
}

/// Returns the namespace prefix of an encoded storage key.
///
/// A [`StorageKey`] is encoded as `contract id (i32, little-endian)` followed by
/// the contract-local key. The first suffix byte is the storage prefix inside
/// that contract (see [`StorageKey::PREFIX_LENGTH`]), so the namespace a scan may
/// cross is `id` plus that one byte. Scanning on the whole current key would only
/// ever match keys that extend it byte for byte.
pub(super) fn namespace_prefix(key: &StorageKey) -> StorageKey {
    let suffix = key.suffix();
    let prefix_bytes = suffix
        .len()
        .min(StorageKey::PREFIX_LENGTH - std::mem::size_of::<i32>());
    StorageKey::new(key.id(), suffix[..prefix_bytes].to_vec())
}

/// Tracks access patterns for intelligent prefetching.
pub(super) struct AccessPatternTracker {
    /// Last accessed key (for pattern detection)
    last_key: Option<StorageKey>,
    /// Last access sequence number
    last_seq: u64,
    /// Detected pattern
    pattern: PrefetchPattern,
    /// Confidence score (0-100)
    confidence: u8,
    /// Sequential access counter
    sequential_count: u32,
}

impl AccessPatternTracker {
    pub(super) fn new() -> Self {
        Self {
            last_key: None,
            last_seq: 0,
            pattern: PrefetchPattern::None,
            confidence: 0,
            sequential_count: 0,
        }
    }

    /// Records an access and returns a scan plan when prefetching is warranted.
    ///
    /// A plan requires a confident sequential pattern *and* enough consecutive
    /// observations of it, so a single lucky ordering step cannot start a scan.
    pub(super) fn record_access(
        &mut self,
        key: &StorageKey,
        seq: u64,
        config: &PrefetchConfig,
    ) -> Option<PrefetchScanPlan> {
        // A sequence that moved backwards cannot be trusted: the counter was
        // reset elsewhere and this access starts a fresh observation window.
        if seq <= self.last_seq {
            self.reset_pattern();
        }

        if let Some(last) = self.last_key.as_ref() {
            if namespace_prefix(last) != namespace_prefix(key) {
                // Ordering across namespaces says nothing about locality inside
                // either of them, so the current run ends here.
                self.reset_pattern();
            } else if *key > *last {
                self.observe(PrefetchPattern::SequentialForward);
            } else if *key < *last {
                self.observe(PrefetchPattern::SequentialBackward);
            } else {
                self.confidence = self.confidence.saturating_sub(5);
                if self.confidence < 10 {
                    self.pattern = PrefetchPattern::None;
                    self.sequential_count = 0;
                }
            }
        }

        self.last_key = Some(key.clone());
        self.last_seq = seq;

        let direction = self.pattern.direction()?;
        if !config.enabled {
            return None;
        }
        if self.confidence < config.confidence_threshold {
            return None;
        }
        if self.sequential_count < config.min_observations {
            return None;
        }

        Some(PrefetchScanPlan {
            direction,
            prefix: namespace_prefix(key),
            start: key.clone(),
        })
    }

    fn observe(&mut self, pattern: PrefetchPattern) {
        if self.pattern == pattern {
            self.sequential_count = self.sequential_count.saturating_add(1);
            self.confidence = self.confidence.saturating_add(10).min(100);
        } else {
            self.pattern = pattern;
            self.sequential_count = 1;
            self.confidence = 20;
        }
    }

    fn reset_pattern(&mut self) {
        self.pattern = PrefetchPattern::None;
        self.confidence = 0;
        self.sequential_count = 0;
    }

    /// Gets the current detected pattern if confidence is high enough.
    pub(super) fn current_pattern(&self, threshold: u8) -> PrefetchPattern {
        if self.confidence >= threshold {
            self.pattern
        } else {
            PrefetchPattern::None
        }
    }

    #[allow(dead_code)]
    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: i32, suffix: &[u8]) -> StorageKey {
        StorageKey::new(id, suffix.to_vec())
    }

    fn enabled_config() -> PrefetchConfig {
        PrefetchConfig {
            enabled: true,
            count: 4,
            confidence_threshold: 30,
            min_observations: 2,
            ..PrefetchConfig::default()
        }
    }

    #[test]
    fn default_configuration_is_disabled() {
        let config = PrefetchConfig::default();
        assert!(!config.enabled);
        assert!(!config.is_active());
        assert!(!PrefetchConfig::disabled().is_active());
        // Counts and limits stay declared so an operator only flips `enabled`.
        assert!(config.count > 0);
        assert!(config.limits.max_scan_keys > 0);
    }

    #[test]
    fn namespace_prefix_keeps_only_contract_and_storage_prefix() {
        let current = key(7, &[0x14, 0xaa, 0xbb, 0xcc]);
        let prefix = namespace_prefix(&current);
        assert_eq!(prefix, key(7, &[0x14]));
        assert_ne!(prefix, current);

        // A key without a suffix has no storage prefix, so the contract id alone
        // is the widest namespace that still cannot cross into another contract.
        assert_eq!(namespace_prefix(&key(7, &[])), key(7, &[]));
    }

    #[test]
    fn plan_requires_confidence_and_observations() {
        let mut tracker = AccessPatternTracker::new();
        let config = enabled_config();

        assert!(
            tracker
                .record_access(&key(1, &[1, 1]), 1, &config)
                .is_none()
        );
        // One forward step reaches confidence 20, below the threshold.
        assert!(
            tracker
                .record_access(&key(1, &[1, 2]), 2, &config)
                .is_none()
        );
        let plan = tracker
            .record_access(&key(1, &[1, 3]), 3, &config)
            .expect("two observations at confidence 30 trigger a scan");
        assert_eq!(plan.direction, SeekDirection::Forward);
        assert_eq!(plan.prefix, key(1, &[1]));
        assert_eq!(plan.start, key(1, &[1, 3]));
    }

    #[test]
    fn namespace_change_resets_the_sequence() {
        let mut tracker = AccessPatternTracker::new();
        let config = enabled_config();

        tracker.record_access(&key(1, &[1, 1]), 1, &config);
        tracker.record_access(&key(1, &[1, 2]), 2, &config);
        // Different storage prefix byte: the run must not carry over.
        assert!(
            tracker
                .record_access(&key(1, &[2, 1]), 3, &config)
                .is_none()
        );
        assert!(
            tracker
                .record_access(&key(1, &[2, 2]), 4, &config)
                .is_none()
        );
        assert!(
            tracker
                .record_access(&key(1, &[2, 3]), 5, &config)
                .is_some()
        );
    }

    #[test]
    fn disabled_configuration_never_produces_a_plan() {
        let mut tracker = AccessPatternTracker::new();
        let config = PrefetchConfig {
            enabled: false,
            confidence_threshold: 0,
            min_observations: 0,
            ..enabled_config()
        };

        for step in 0..8u8 {
            assert!(
                tracker
                    .record_access(&key(1, &[1, step]), u64::from(step), &config)
                    .is_none()
            );
        }
    }

    #[test]
    fn window_is_bounded_and_keeps_the_newest_keys() {
        let mut window = PrefetchWindow::new(2);
        window.record(key(1, &[1]));
        window.record(key(1, &[2]));
        window.record(key(1, &[3]));

        assert_eq!(window.len(), 2);
        assert!(!window.contains(&key(1, &[1])));
        assert!(window.contains(&key(1, &[2])));
        assert!(window.contains(&key(1, &[3])));

        window.clear();
        assert!(!window.contains(&key(1, &[3])));
    }

    #[test]
    fn admission_target_respects_the_admit_ceiling() {
        let mut config = enabled_config();
        config.count = 32;
        config.limits.max_admit_keys = 5;
        assert_eq!(config.admit_target(), 5);

        config.count = 3;
        assert_eq!(config.admit_target(), 3);
    }
}
