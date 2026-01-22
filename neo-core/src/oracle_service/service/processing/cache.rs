use super::super::{OracleService, FINISHED_CACHE_TTL};
use crate::persistence::{DataCache, StoreCache};
use crate::smart_contract::native::OracleContract;
use std::collections::HashSet;
use std::time::SystemTime;

impl OracleService {
    pub(in super::super) fn sync_pending_queue(&self, snapshot: &DataCache) {
        let offchain = OracleContract::new()
            .get_requests(snapshot)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect::<HashSet<_>>();

        let mut queue = self.pending_queue.lock();
        queue.retain(|id, _| offchain.contains(id));
    }

    pub(in super::super) fn is_request_finished(&self, request_id: u64) -> bool {
        self.finished_cache.lock().contains_key(&request_id)
    }

    pub(in super::super) fn cleanup_finished_cache(&self, now: SystemTime) {
        let mut cache = self.finished_cache.lock();
        cache.retain(|_, timestamp| {
            if let Ok(span) = now.duration_since(*timestamp) {
                span <= FINISHED_CACHE_TTL
            } else {
                true
            }
        });
    }

    pub(in super::super) fn snapshot_cache(&self) -> DataCache {
        let snapshot = self.system.store().get_snapshot();
        let store_cache = StoreCache::new_from_snapshot(snapshot);
        store_cache.data_cache().clone()
    }
}
