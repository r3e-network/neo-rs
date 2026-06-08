use crate::types::{StorageItem, StorageKey, TrackState};

/// No-op implementation of watched storage event logging.
///
/// The full implementation with `UInt160`/`UInt256` context tracking
/// lives in neo-core's `data_cache::storage_watch` module behind the `runtime` feature.
pub(super) fn log_watched_storage_event(
    _op: &'static str,
    _source: &'static str,
    _key: &StorageKey,
    _prev_state: Option<TrackState>,
    _new_state: Option<TrackState>,
    _value: Option<&StorageItem>,
) {
}
