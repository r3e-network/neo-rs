use super::super::OracleService;
use super::super::cache::ExpiryBoundary;
use super::super::native_provider::OracleServiceNativeProvider;
use neo_storage::persistence::{DataCache, StoreCache};
use std::collections::HashSet;
use std::time::SystemTime;

impl OracleService {
    pub(in super::super) fn sync_pending_queue(&self, snapshot: &DataCache) {
        let native = self.native_provider();
        let Ok(requests) = native.oracle_requests(snapshot) else {
            return;
        };
        let offchain: HashSet<u64> = requests.into_iter().map(|(id, _)| id).collect();

        let mut queue = self.pending_queue.lock();
        queue.retain(|id, _| offchain.contains(id));
    }

    pub(in super::super) fn is_request_finished(&self, request_id: u64) -> bool {
        self.finished_cache.lock().contains(&request_id)
    }

    pub(in super::super) fn cleanup_finished_cache(&self, now: SystemTime) {
        self.finished_cache
            .lock()
            .prune_expired(now, ExpiryBoundary::Inclusive);
    }

    pub(in super::super) fn snapshot_cache(&self) -> DataCache {
        let snapshot = self.store.store().snapshot();
        let store_cache = StoreCache::new_from_snapshot(snapshot);
        store_cache.data_cache().clone()
    }
}
