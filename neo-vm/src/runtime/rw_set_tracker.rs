//! Read-write set tracking for optimistically parallel transaction execution.
//!
//! A [`RwSetTracker`] records the state keys a single transaction observed and
//! modified, so a scheduler can decide - without re-executing it - whether a
//! later transaction invalidated those observations.
//!
//! Recording is opt-in and owned by the application engine: no tracker is
//! allocated at all unless the host enables it (see
//! `ApplicationEngine::enable_rw_set_tracking`), therefore the default
//! execution path pays neither allocations nor bookkeeping.
//!
//! # Recorded observations
//!
//! * exact reads - including reads that returned no value ([`RwSetTracker::record_absent_read`]).
//!   An absent read is still a dependency: if a later transaction creates that
//!   key, the absence observation is invalidated.
//! * exact writes - `Put` and `Delete` both write; a delete is not a read.
//! * prefix reads - `Find` enumerates a range, so any later write whose key
//!   starts with the scanned prefix invalidates the scan.
//! * notification and log emissions - not conflict keys, but the scheduler uses
//!   them as ordering barriers because events are observable outside the set.
//!
//! # Versions
//!
//! Every tracker carries a `generation` (the version of the state it started
//! from) and a local write clock that stamps each write with a strictly larger
//! version. Reads remember the version they observed, which lets a committed
//! tracker built by [`RwSetTracker::merge_committed`] tell two cases apart:
//!
//! * `read .. committed write` - the read saw the older value, so it is stale
//!   (see [`RwSetTracker::stale_read_keys`]);
//! * `committed write .. read` - the read already observed that write, so it is
//!   still valid and re-execution is unnecessary.
//!
//! Both sides must share a version lineage. The validator stamps each
//! transaction's tracker with `set_generation(committed.write_clock())` before
//! execution, and `merge_committed` advances the committed clock past the
//! merged set.

use std::collections::{HashMap, HashSet};

use neo_primitives::UInt256;

/// A logical state key.
///
/// Keys are opaque byte strings: the virtual machine only needs equality and
/// prefix relations, so engine layers decide the encoding (contract storage
/// keys are `contract id || key suffix`).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RwKey(Vec<u8>);

impl RwKey {
    /// Wraps raw bytes as a key.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Returns the raw key bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the key length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the key has no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns whether this key starts with `prefix`.
    #[must_use]
    pub fn starts_with(&self, prefix: &[u8]) -> bool {
        self.0.starts_with(prefix)
    }

    /// Unwraps the key into its raw bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for RwKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for RwKey {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for RwKey {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl From<&Vec<u8>> for RwKey {
    fn from(value: &Vec<u8>) -> Self {
        Self(value.clone())
    }
}

impl From<UInt256> for RwKey {
    fn from(value: UInt256) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

impl From<RwKey> for Vec<u8> {
    fn from(value: RwKey) -> Self {
        value.0
    }
}

/// Records the read set, write set and dependency versions of one transaction.
///
/// Conflict detection is set based (`HashSet` intersection), so a check costs
/// `O(min(|a|, |b|))` rather than `O(|a| * |b|)` as a `Vec::contains` scan would.
#[derive(Clone, Debug, Default)]
pub struct RwSetTracker {
    read_keys: HashSet<RwKey>,
    write_keys: HashSet<RwKey>,
    absent_reads: HashSet<RwKey>,
    read_prefixes: HashSet<RwKey>,
    read_versions: HashMap<RwKey, u64>,
    write_versions: HashMap<RwKey, u64>,
    generation: u64,
    write_clock: u64,
    access_count: usize,
    /// Keys of `Notify` events emitted during execution.
    pub notify_events: Vec<RwKey>,
    /// Keys of `Log` events emitted during execution.
    pub log_events: Vec<RwKey>,
    /// Gas consumed during execution.
    pub gas_used: i64,
}

impl RwSetTracker {
    /// Creates an empty tracker positioned at generation zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty tracker positioned at the given state generation.
    #[must_use]
    pub fn with_generation(generation: u64) -> Self {
        Self {
            generation,
            write_clock: generation,
            ..Self::default()
        }
    }

    /// Returns every key read by this transaction, absent reads included.
    #[must_use]
    pub fn read_set(&self) -> &HashSet<RwKey> {
        &self.read_keys
    }

    /// Returns every key written by this transaction.
    #[must_use]
    pub fn write_set(&self) -> &HashSet<RwKey> {
        &self.write_keys
    }

    /// Returns the reads that observed no stored value.
    #[must_use]
    pub fn absent_read_set(&self) -> &HashSet<RwKey> {
        &self.absent_reads
    }

    /// Returns the scanned key prefixes.
    #[must_use]
    pub fn read_prefix_set(&self) -> &HashSet<RwKey> {
        &self.read_prefixes
    }

    /// Returns the version each read observed. Tracked keys only.
    #[must_use]
    pub fn read_versions(&self) -> &HashMap<RwKey, u64> {
        &self.read_versions
    }

    /// Returns the version assigned to each write.
    #[must_use]
    pub fn write_versions(&self) -> &HashMap<RwKey, u64> {
        &self.write_versions
    }

    /// Returns the generation this set was produced from.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the newest version stamped by this tracker.
    ///
    /// The next transaction validated against this set must start from it, so
    /// that the versions of both sides stay comparable.
    #[must_use]
    pub fn write_clock(&self) -> u64 {
        self.write_clock
    }

    /// Positions this set at the given state generation.
    ///
    /// The write clock is raised to at least `generation`, which keeps every
    /// write stamped after the state this tracker started from.
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.write_clock = self.write_clock.max(generation);
    }

    /// Records a key read that returned a value.
    ///
    /// The first read of a key fixes the version it observed; later reads of the
    /// same key do not move that version.
    pub fn record_read(&mut self, key: impl Into<RwKey>) {
        let key = key.into();
        self.access_count = self.access_count.saturating_add(1);
        if self.read_keys.insert(key.clone()) {
            let observed = self
                .write_versions
                .get(&key)
                .copied()
                .unwrap_or(self.generation);
            self.read_versions.insert(key, observed);
        }
    }

    /// Records a key read that returned no value.
    ///
    /// An absent read is a dependency on the *absence* of the key, so a later
    /// transaction creating it conflicts with this one.
    pub fn record_absent_read(&mut self, key: impl Into<RwKey>) {
        let key = key.into();
        self.absent_reads.insert(key.clone());
        self.record_read(key);
    }

    /// Records a key written (created, updated or deleted) by this transaction.
    pub fn record_write(&mut self, key: impl Into<RwKey>) {
        let key = key.into();
        self.access_count = self.access_count.saturating_add(1);
        self.write_clock = self.write_clock.saturating_add(1);
        self.write_versions.insert(key.clone(), self.write_clock);
        self.write_keys.insert(key);
    }

    /// Records a prefix scan (`Find`) as a range read.
    ///
    /// The keys the scan actually returned should also be recorded as exact
    /// reads; the prefix only guards against keys created afterwards.
    pub fn record_read_prefix(&mut self, prefix: impl Into<RwKey>) {
        let prefix = prefix.into();
        self.access_count = self.access_count.saturating_add(1);
        self.read_prefixes.insert(prefix);
    }

    /// Records a contract notification key (ordering barrier, not a conflict key).
    pub fn record_notify_event(&mut self, key: impl Into<RwKey>) {
        self.notify_events.push(key.into());
    }

    /// Records a contract log key (ordering barrier, not a conflict key).
    pub fn record_log_event(&mut self, key: impl Into<RwKey>) {
        self.log_events.push(key.into());
    }

    /// Sets the gas consumed during execution.
    pub fn set_gas_used(&mut self, gas: i64) {
        self.gas_used = gas;
    }

    /// Returns whether this tracker recorded any writes.
    #[must_use]
    pub fn has_writes(&self) -> bool {
        !self.write_keys.is_empty()
    }

    /// Returns whether this tracker recorded any reads.
    #[must_use]
    pub fn has_reads(&self) -> bool {
        !self.read_keys.is_empty()
    }

    /// Returns whether any read observed a missing value.
    #[must_use]
    pub fn has_absent_reads(&self) -> bool {
        !self.absent_reads.is_empty()
    }

    /// Returns whether any key range was scanned.
    #[must_use]
    pub fn has_read_prefixes(&self) -> bool {
        !self.read_prefixes.is_empty()
    }

    /// Returns whether any notification or log was emitted.
    #[must_use]
    pub fn has_events(&self) -> bool {
        !self.notify_events.is_empty() || !self.log_events.is_empty()
    }

    /// Returns whether nothing at all was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read_keys.is_empty()
            && self.write_keys.is_empty()
            && self.read_prefixes.is_empty()
            && !self.has_events()
    }

    /// Total number of accesses recorded so far, counting repeated accesses.
    #[must_use]
    pub fn total_accesses(&self) -> usize {
        self.access_count
    }

    /// Returns whether `key` was read by this transaction.
    #[must_use]
    pub fn read(&self, key: &RwKey) -> bool {
        self.read_keys.contains(key)
    }

    /// Returns whether `key` was written by this transaction.
    #[must_use]
    pub fn wrote(&self, key: &RwKey) -> bool {
        self.write_keys.contains(key)
    }

    /// Directional conflict check: does anything this tracker writes invalidate
    /// the other tracker?
    ///
    /// * write-write: both write the same key -> conflict;
    /// * write-read: we write what the other reads (exact key or scanned prefix) -> conflict;
    /// * read-write: the other writes what we read -> not our conflict, the
    ///   stale read belongs to the other side.
    #[must_use]
    pub fn conflicts_with(&self, other: &RwSetTracker) -> bool {
        sets_intersect(&self.write_keys, &other.read_keys)
            || sets_intersect(&self.write_keys, &other.write_keys)
            || writes_match_prefixes(&self.write_keys, &other.read_prefixes)
    }

    /// Symmetric conflict check between two transaction read-write sets.
    ///
    /// Unlike [`Self::conflicts_with`], a conflict is reported in either
    /// direction, which is what a scheduler needs to tell whether two
    /// transactions may run concurrently at all.
    #[must_use]
    pub fn has_conflict(&self, other: &RwSetTracker) -> bool {
        self.conflicts_with(other) || other.conflicts_with(self)
    }

    /// Returns the reads this tracker made before the committed tracker wrote
    /// the same key - i.e. the observations the committed writes invalidated.
    ///
    /// Keys are only comparable when both trackers share a version lineage:
    /// stamp this tracker with `set_generation(committed.write_clock())` before
    /// execution and keep the committed side up to date with
    /// [`Self::merge_committed`]. Empty means every read still observes the
    /// committed state, so the transaction does not need to be re-executed.
    #[must_use]
    pub fn stale_read_keys(&self, committed: &RwSetTracker) -> Vec<RwKey> {
        let mut stale = Vec::new();
        for (key, observed) in &self.read_versions {
            if let Some(written) = committed.write_versions.get(key)
                && written > observed
            {
                stale.push(key.clone());
            }
        }
        if !self.read_prefixes.is_empty() {
            for prefix in &self.read_prefixes {
                let superseded = committed.write_versions.iter().any(|(key, written)| {
                    *written > self.generation && key.starts_with(prefix.as_bytes())
                });
                if superseded {
                    stale.push(prefix.clone());
                }
            }
        }
        stale.sort();
        stale.dedup();
        stale
    }

    /// Returns whether any read is stale against the committed tracker.
    #[must_use]
    pub fn has_stale_reads(&self, committed: &RwSetTracker) -> bool {
        !self.stale_read_keys(committed).is_empty()
    }

    /// Folds another tracker's writes into this one, building the committed
    /// state incrementally during ordered validation.
    ///
    /// The merged write versions are kept, so the next transaction can be
    /// stamped with `set_generation(committed.write_clock())` and compared
    /// against them. Reads of the merged tracker are not extended: the committed
    /// state only needs to answer "which keys changed, and when".
    pub fn merge_committed(&mut self, other: &RwSetTracker) {
        for key in &other.write_keys {
            let version = other
                .write_versions
                .get(key)
                .copied()
                .unwrap_or(other.generation);
            let entry = self.write_versions.entry(key.clone()).or_insert(version);
            if version > *entry {
                *entry = version;
            }
            self.write_keys.insert(key.clone());
        }
        self.write_clock = self.write_clock.max(other.write_clock);
        self.generation = self.generation.max(other.generation);
    }

    /// Clears every observation, keeping the generation this tracker belongs to.
    pub fn clear(&mut self) {
        self.read_keys.clear();
        self.write_keys.clear();
        self.absent_reads.clear();
        self.read_prefixes.clear();
        self.read_versions.clear();
        self.write_versions.clear();
        self.access_count = 0;
        self.write_clock = self.generation;
        self.notify_events.clear();
        self.log_events.clear();
        self.gas_used = 0;
    }

    /// Reserves read-key capacity to avoid reallocation during execution.
    pub fn reserve_read_capacity(&mut self, additional: usize) {
        self.read_keys.reserve(additional);
        self.read_versions.reserve(additional);
    }
}

fn sets_intersect(left: &HashSet<RwKey>, right: &HashSet<RwKey>) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left.len() <= right.len() {
        left.iter().any(|key| right.contains(key))
    } else {
        right.iter().any(|key| left.contains(key))
    }
}

fn writes_match_prefixes(writes: &HashSet<RwKey>, prefixes: &HashSet<RwKey>) -> bool {
    if writes.is_empty() || prefixes.is_empty() {
        return false;
    }
    prefixes
        .iter()
        .any(|prefix| writes.iter().any(|key| key.starts_with(prefix.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> RwKey {
        RwKey::new(vec![n; 32])
    }

    fn tracker(reads: &[u8], writes: &[u8]) -> RwSetTracker {
        let mut result = RwSetTracker::new();
        for &read in reads {
            result.record_read(key(read));
        }
        for &write in writes {
            result.record_write(key(write));
        }
        result
    }

    #[test]
    fn records_reads_and_writes_without_duplicates() {
        let mut set = RwSetTracker::new();
        set.record_read(key(1));
        set.record_read(key(1));
        set.record_write(key(2));
        set.record_write(key(2));

        assert_eq!(set.read_set().len(), 1);
        assert_eq!(set.write_set().len(), 1);
        assert_eq!(set.total_accesses(), 4);
        assert!(set.read(&key(1)));
        assert!(set.wrote(&key(2)));
        assert!(set.has_reads() && set.has_writes());
    }

    #[test]
    fn disjoint_sets_do_not_conflict() {
        let a = tracker(&[1, 2], &[3, 4]);
        let b = tracker(&[5, 6], &[7, 8]);

        assert!(!a.has_conflict(&b));
        assert!(!b.has_conflict(&a));
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn write_read_conflict_is_directional_for_conflicts_with() {
        let writer = tracker(&[], &[1]);
        let reader = tracker(&[1], &[]);

        assert!(writer.conflicts_with(&reader));
        assert!(!reader.conflicts_with(&writer));
        assert!(reader.has_conflict(&writer));
        assert!(writer.has_conflict(&reader));
    }

    #[test]
    fn write_write_conflict() {
        let a = tracker(&[], &[1]);
        let b = tracker(&[], &[1]);

        assert!(a.has_conflict(&b));
        assert!(a.conflicts_with(&b));
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn absent_read_conflicts_with_later_creation() {
        let mut reader = RwSetTracker::new();
        reader.record_absent_read(key(9));
        let writer = tracker(&[], &[9]);

        assert!(reader.has_absent_reads());
        assert!(reader.read(&key(9)));
        assert!(writer.conflicts_with(&reader));
        assert!(reader.has_conflict(&writer));

        let unrelated = tracker(&[], &[10]);
        assert!(!reader.has_conflict(&unrelated));
    }

    #[test]
    fn prefix_read_conflicts_with_matching_write_only() {
        let mut scanner = RwSetTracker::new();
        scanner.record_read_prefix(vec![0xAA]);
        scanner.record_read(RwKey::new(vec![0xAA, 0x01]));

        let mut matching = RwSetTracker::new();
        matching.record_write(RwKey::new(vec![0xAA, 0x02]));
        assert!(scanner.has_conflict(&matching));

        let mut non_matching = RwSetTracker::new();
        non_matching.record_write(RwKey::new(vec![0xAB, 0x02]));
        assert!(!scanner.has_conflict(&non_matching));
        assert!(scanner.has_read_prefixes());
    }

    #[test]
    fn prefix_read_of_wide_range_matches_nested_keys() {
        let mut scanner = RwSetTracker::new();
        scanner.record_read_prefix(vec![0x10, 0x20]);

        let mut writer = RwSetTracker::new();
        writer.record_write(RwKey::new(vec![0x10, 0x20, 0x30, 0x40]));

        assert!(scanner.has_conflict(&writer));
        assert!(writer.has_conflict(&scanner));
    }

    #[test]
    fn events_are_not_conflict_keys() {
        let mut a = RwSetTracker::new();
        a.record_notify_event(key(1));
        a.record_log_event(key(1));

        let b = tracker(&[], &[1]);
        assert!(!a.has_conflict(&b));
        assert!(a.has_events());
        assert!(!a.is_empty());
    }

    #[test]
    fn write_versions_increase_and_reads_observe_them() {
        let mut set = RwSetTracker::with_generation(5);
        set.record_write(key(1));
        set.record_read(key(1));

        assert_eq!(set.read_versions().get(&key(1)), Some(&6));
        assert_eq!(set.write_versions().get(&key(1)), Some(&6));
        assert_eq!(set.write_clock(), 6);
        assert_eq!(set.generation(), 5);

        set.record_write(key(2));
        assert_eq!(set.write_versions().get(&key(2)), Some(&7));
    }

    #[test]
    fn stale_read_detects_read_before_committed_write() {
        // The transaction executed against generation 0 and read key 1.
        let mut candidate = RwSetTracker::with_generation(0);
        candidate.record_read(key(1));

        // A predecessor committed a write to key 1 at version 1.
        let mut committed = RwSetTracker::new();
        committed.record_write(key(1));

        assert_eq!(candidate.stale_read_keys(&committed), vec![key(1)]);
        assert!(candidate.has_stale_reads(&committed));
    }

    #[test]
    fn read_after_committed_write_is_not_stale() {
        let mut committed = RwSetTracker::new();
        committed.record_write(key(1));

        // The transaction started from the committed clock and saw that write.
        let mut candidate = RwSetTracker::with_generation(committed.write_clock());
        candidate.record_read(key(1));

        assert!(candidate.stale_read_keys(&committed).is_empty());
        assert!(!candidate.has_stale_reads(&committed));
    }

    #[test]
    fn stale_prefix_read_detects_later_creation_in_range() {
        let mut candidate = RwSetTracker::with_generation(0);
        candidate.record_read_prefix(vec![0x01]);
        candidate.record_read(RwKey::new(vec![0x01, 0xAA]));

        let mut committed = RwSetTracker::new();
        committed.record_write(RwKey::new(vec![0x01, 0xBB]));

        assert_eq!(
            candidate.stale_read_keys(&committed),
            vec![RwKey::new(vec![0x01])]
        );
    }

    #[test]
    fn merge_committed_accumulates_writes_and_clock() {
        let mut committed = RwSetTracker::new();
        let first = tracker(&[1], &[10]);
        let second = tracker(&[], &[11]);

        committed.merge_committed(&first);
        committed.merge_committed(&second);

        assert!(committed.wrote(&key(10)));
        assert!(committed.wrote(&key(11)));
        assert_eq!(committed.write_set().len(), 2);
        assert_eq!(committed.write_clock(), second.write_clock());

        // The merged set answers structural conflict questions directly.
        let later = tracker(&[11], &[]);
        assert!(committed.has_conflict(&later));
    }

    #[test]
    fn merge_committed_keeps_highest_version_per_key() {
        let mut committed = RwSetTracker::new();
        let mut early = RwSetTracker::with_generation(0);
        early.record_write(key(1));
        let mut late = RwSetTracker::with_generation(10);
        late.record_write(key(1));
        late.record_write(key(2));

        committed.merge_committed(&late);
        committed.merge_committed(&early);

        assert_eq!(committed.write_versions().get(&key(1)), Some(&11));
        assert_eq!(committed.write_clock(), 12);
    }

    #[test]
    fn set_generation_raises_write_clock() {
        let mut set = RwSetTracker::new();
        set.set_generation(42);
        set.record_write(key(1));

        assert_eq!(set.generation(), 42);
        assert_eq!(set.write_versions().get(&key(1)), Some(&43));
    }

    #[test]
    fn clear_resets_observations_but_keeps_generation() {
        let mut set = RwSetTracker::with_generation(7);
        set.record_read(key(1));
        set.record_absent_read(key(2));
        set.record_write(key(3));
        set.record_read_prefix(vec![0x04]);
        set.record_notify_event(key(5));
        set.record_log_event(key(6));
        set.set_gas_used(11);
        set.reserve_read_capacity(16);

        set.clear();

        assert!(!set.has_reads() && !set.has_writes());
        assert!(!set.has_absent_reads() && !set.has_read_prefixes());
        assert!(!set.has_events());
        assert_eq!(set.total_accesses(), 0);
        assert_eq!(set.gas_used, 0);
        assert!(set.is_empty());
        assert_eq!(set.generation(), 7);
        assert_eq!(set.write_clock(), 7);
    }

    #[test]
    fn empty_trackers_never_conflict() {
        let empty = RwSetTracker::new();
        assert!(empty.is_empty());
        assert!(!empty.has_conflict(&empty));
        assert!(empty.stale_read_keys(&empty).is_empty());
    }

    #[test]
    fn has_conflict_uses_exact_key_equality() {
        let mut a = RwSetTracker::new();
        a.record_write(RwKey::new(vec![0x01]));
        let mut b = RwSetTracker::new();
        b.record_write(RwKey::new(vec![0x01, 0x00]));

        // A prefix is not an equality match: only scanned ranges are prefixes.
        assert!(!a.has_conflict(&b));
    }
}
