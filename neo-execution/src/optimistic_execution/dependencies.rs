use neo_storage::{
    DataCacheReadObserver, DataCacheReadOrigin, SeekDirection, StorageItem, StorageKey,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Default maximum number of distinct pinned-prefix point reads per transaction.
pub const DEFAULT_MAX_POINT_READ_DEPENDENCIES: usize = 65_536;

/// Default maximum serialized key and value bytes retained per transaction.
pub const DEFAULT_MAX_POINT_READ_DEPENDENCY_BYTES: usize = 64 * 1024 * 1024;

/// Hard memory bounds for one opt-in speculative dependency capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DependencyCaptureLimits {
    max_point_reads: usize,
    max_captured_bytes: usize,
}

impl DependencyCaptureLimits {
    /// Creates explicit per-transaction capture limits.
    #[must_use]
    pub const fn new(max_point_reads: usize, max_captured_bytes: usize) -> Self {
        Self {
            max_point_reads,
            max_captured_bytes,
        }
    }

    /// Maximum number of distinct point-read dependencies.
    #[must_use]
    pub const fn max_point_reads(self) -> usize {
        self.max_point_reads
    }

    /// Maximum total serialized key and value bytes.
    #[must_use]
    pub const fn max_captured_bytes(self) -> usize {
        self.max_captured_bytes
    }
}

impl Default for DependencyCaptureLimits {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_POINT_READ_DEPENDENCIES,
            DEFAULT_MAX_POINT_READ_DEPENDENCY_BYTES,
        )
    }
}

/// One exact present or absent read from the transaction's pinned prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointReadDependency {
    key: StorageKey,
    value: Option<StorageItem>,
}

impl PointReadDependency {
    /// Exact storage key observed by the transaction.
    #[must_use]
    pub const fn key(&self) -> &StorageKey {
        &self.key
    }

    /// Exact visible value, or `None` when the key was absent.
    #[must_use]
    pub const fn value(&self) -> Option<&StorageItem> {
        self.value.as_ref()
    }
}

/// Deterministically ordered point dependencies captured for one transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionDependencies {
    point_reads: Vec<PointReadDependency>,
    captured_bytes: usize,
}

impl TransactionDependencies {
    /// Point reads ordered by canonical serialized storage-key bytes.
    #[must_use]
    pub fn point_reads(&self) -> &[PointReadDependency] {
        &self.point_reads
    }

    /// Total serialized key and value bytes retained by this snapshot.
    #[must_use]
    pub const fn captured_bytes(&self) -> usize {
        self.captured_bytes
    }
}

/// Fail-closed reason that prevents a speculative dependency set from use.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DependencyCaptureError {
    /// Range validation is a later gate; iterators must currently run sequentially.
    #[error("storage range dependency capture is not yet supported")]
    UnsupportedRangeRead {
        /// Exact prefix, or `None` for a whole-store traversal.
        prefix: Option<StorageKey>,
        /// Traversal direction used by the read.
        direction: SeekDirection,
    },
    /// The transaction exceeded its distinct point-read bound.
    #[error("point-read dependency limit {limit} exceeded")]
    PointReadLimitExceeded {
        /// Configured maximum number of distinct point reads.
        limit: usize,
    },
    /// The transaction exceeded its retained dependency-byte bound.
    #[error(
        "point-read dependency byte limit {limit} exceeded by attempted size {attempted_bytes}"
    )]
    CapturedBytesLimitExceeded {
        /// Configured maximum retained bytes.
        limit: usize,
        /// Size that would have resulted from the rejected first read.
        attempted_bytes: usize,
    },
}

#[derive(Default)]
struct DependencyCaptureState {
    point_reads: BTreeMap<StorageKey, Option<StorageItem>>,
    captured_bytes: usize,
    failure: Option<DependencyCaptureError>,
    sealed: bool,
}

struct DependencyRecorder {
    limits: DependencyCaptureLimits,
    state: Mutex<DependencyCaptureState>,
}

impl DependencyRecorder {
    fn record_point(
        &self,
        key: &StorageKey,
        value: Option<&StorageItem>,
        origin: DataCacheReadOrigin,
    ) {
        if origin == DataCacheReadOrigin::Overlay {
            return;
        }

        let mut state = self.state.lock();
        if state.sealed || state.failure.is_some() || state.point_reads.contains_key(key) {
            return;
        }
        if state.point_reads.len() >= self.limits.max_point_reads {
            state.failure = Some(DependencyCaptureError::PointReadLimitExceeded {
                limit: self.limits.max_point_reads,
            });
            return;
        }

        let value_bytes = value.map_or(0, |item| item.value_bytes().len());
        let attempted_bytes = state
            .captured_bytes
            .saturating_add(key.length())
            .saturating_add(value_bytes);
        if attempted_bytes > self.limits.max_captured_bytes {
            state.failure = Some(DependencyCaptureError::CapturedBytesLimitExceeded {
                limit: self.limits.max_captured_bytes,
                attempted_bytes,
            });
            return;
        }

        state.captured_bytes = attempted_bytes;
        state.point_reads.insert(key.clone(), value.cloned());
    }

    fn mark_range_unsupported(&self, prefix: Option<&StorageKey>, direction: SeekDirection) {
        let mut state = self.state.lock();
        if !state.sealed && state.failure.is_none() {
            state.failure = Some(DependencyCaptureError::UnsupportedRangeRead {
                prefix: prefix.cloned(),
                direction,
            });
        }
    }
}

impl DataCacheReadObserver for DependencyRecorder {
    fn observe_point_read(&self, key: &StorageKey, value: Option<&StorageItem>) {
        self.record_point(key, value, DataCacheReadOrigin::PinnedPrefix);
    }

    fn observe_point_read_with_origin(
        &self,
        key: &StorageKey,
        value: Option<&StorageItem>,
        origin: DataCacheReadOrigin,
    ) {
        self.record_point(key, value, origin);
    }

    fn observe_range_read(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
        _rows: &[(StorageKey, StorageItem)],
    ) {
        self.mark_range_unsupported(prefix, direction);
    }
}

/// Shared handle for inspecting one opt-in speculative dependency capture.
#[derive(Clone)]
pub struct TransactionDependencyCapture {
    recorder: Arc<DependencyRecorder>,
}

impl TransactionDependencyCapture {
    pub(super) fn new(limits: DependencyCaptureLimits) -> Self {
        Self {
            recorder: Arc::new(DependencyRecorder {
                limits,
                state: Mutex::new(DependencyCaptureState::default()),
            }),
        }
    }

    pub(super) fn observer(&self) -> Arc<dyn DataCacheReadObserver> {
        self.recorder.clone()
    }

    pub(super) fn same_capture(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.recorder, &other.recorder)
    }

    /// Permanently closes the recorder and waits for an in-flight observation.
    pub(super) fn seal(&self) {
        self.recorder.state.lock().sealed = true;
    }

    /// Copies the current deterministic dependency set or its first fail-closed reason.
    pub fn snapshot(&self) -> Result<TransactionDependencies, DependencyCaptureError> {
        let state = self.recorder.state.lock();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        Ok(TransactionDependencies {
            point_reads: state
                .point_reads
                .iter()
                .map(|(key, value)| PointReadDependency {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            captured_bytes: state.captured_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_keeps_first_pinned_value_and_canonical_key_order() {
        let recorder = DependencyRecorder {
            limits: DependencyCaptureLimits::default(),
            state: Mutex::new(DependencyCaptureState::default()),
        };
        let later_key = StorageKey::new(7, b"z".to_vec());
        let earlier_key = StorageKey::new(7, b"a".to_vec());
        let first = StorageItem::from_bytes(b"first".to_vec());
        let changed = StorageItem::from_bytes(b"changed".to_vec());

        recorder.record_point(&later_key, Some(&first), DataCacheReadOrigin::PinnedPrefix);
        recorder.record_point(
            &later_key,
            Some(&changed),
            DataCacheReadOrigin::PinnedPrefix,
        );
        recorder.record_point(&earlier_key, None, DataCacheReadOrigin::PinnedPrefix);

        let state = recorder.state.lock();
        let dependencies = state.point_reads.iter().collect::<Vec<_>>();
        assert_eq!(dependencies[0], (&earlier_key, &None));
        assert_eq!(dependencies[1], (&later_key, &Some(first)));
    }
}
