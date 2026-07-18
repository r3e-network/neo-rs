use crate::{SeekDirection, StorageItem, StorageKey};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Identifies whether a point-read result came from the pinned transaction
/// prefix or from writes inside the detached transaction overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataCacheReadOrigin {
    /// The value or absence was resolved from state pinned before the transaction.
    PinnedPrefix,
    /// The value or absence was produced by a write in the transaction overlay.
    Overlay,
}

/// Observes final, visible reads performed through a [`super::DataCache`].
///
/// Implementations receive borrowed values so an unobserved cache read retains
/// its ordinary allocation behavior. Observation is opt-in and intended for
/// bounded execution dependency capture, diagnostics, and tests.
pub trait DataCacheReadObserver: Send + Sync + 'static {
    /// Observes one present or absent point read.
    fn observe_point_read(&self, key: &StorageKey, value: Option<&StorageItem>);

    /// Observes a point read together with its transaction-relative origin.
    ///
    /// The default preserves compatibility for observers that do not need
    /// origin classification. New dependency-capture observers should override
    /// this method; caches invoke this hook for every point observation.
    fn observe_point_read_with_origin(
        &self,
        key: &StorageKey,
        value: Option<&StorageItem>,
        _origin: DataCacheReadOrigin,
    ) {
        self.observe_point_read(key, value);
    }

    /// Observes one fully materialized range in traversal order.
    fn observe_range_read(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
        rows: &[(StorageKey, StorageItem)],
    );
}

pub(super) struct DataCacheReadObservation {
    observer: Arc<dyn DataCacheReadObserver>,
    enabled: AtomicBool,
    suspension_depth: AtomicUsize,
    callback_gate: RwLock<()>,
}

impl DataCacheReadObservation {
    pub(super) fn new(observer: Arc<dyn DataCacheReadObserver>) -> Self {
        Self {
            observer,
            enabled: AtomicBool::new(true),
            suspension_depth: AtomicUsize::new(0),
            callback_gate: RwLock::new(()),
        }
    }

    #[inline]
    pub(super) fn observe_point(
        &self,
        key: &StorageKey,
        value: Option<&StorageItem>,
        origin: DataCacheReadOrigin,
    ) {
        if !self.is_active() {
            return;
        }
        let _callback = self.callback_gate.read();
        if self.is_active() {
            self.observer
                .observe_point_read_with_origin(key, value, origin);
        }
    }

    #[inline]
    pub(super) fn observe_range(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
        rows: &[(StorageKey, StorageItem)],
    ) {
        if !self.is_active() {
            return;
        }
        let _callback = self.callback_gate.read();
        if self.is_active() {
            self.observer.observe_range_read(prefix, direction, rows);
        }
    }

    #[inline]
    pub(super) fn is_active(&self) -> bool {
        self.enabled.load(Ordering::Acquire) && self.suspension_depth.load(Ordering::Acquire) == 0
    }

    fn suspend(self: &Arc<Self>) -> DataCacheReadObservationPause {
        let previous = self.suspension_depth.fetch_add(1, Ordering::AcqRel);
        if previous == usize::MAX {
            self.enabled.store(false, Ordering::Release);
            let _in_flight = self.callback_gate.write();
            return DataCacheReadObservationPause { observation: None };
        }
        let _in_flight = self.callback_gate.write();
        DataCacheReadObservationPause {
            observation: Some(Arc::clone(self)),
        }
    }

    pub(super) fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        let _in_flight = self.callback_gate.write();
    }
}

/// RAII pause for one cache observer shared by a cache and all child overlays.
pub struct DataCacheReadObservationPause {
    observation: Option<Arc<DataCacheReadObservation>>,
}

impl Drop for DataCacheReadObservationPause {
    fn drop(&mut self) {
        if let Some(observation) = &self.observation {
            observation.suspension_depth.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl DataCacheReadObservationPause {
    pub(super) fn inactive() -> Self {
        Self { observation: None }
    }
}

pub(super) fn pause(
    observation: Option<&Arc<DataCacheReadObservation>>,
) -> DataCacheReadObservationPause {
    observation.map_or_else(DataCacheReadObservationPause::inactive, |value| {
        value.suspend()
    })
}
