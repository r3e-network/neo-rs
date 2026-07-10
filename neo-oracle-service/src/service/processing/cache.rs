use super::super::cache::ExpiryBoundary;
use super::super::native_provider::{OracleContractReadProvider, OracleServiceNativeProvider};
use super::super::{OracleRuntimeProvider, OracleService};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::Store;
use neo_storage::persistence::{CacheRead, DataCache, StoreCache, StoreDataCache};
use std::collections::HashSet;
use std::time::SystemTime;

impl<R, P> OracleService<R, P>
where
    R: OracleRuntimeProvider + 'static,
    P: NativeContractProvider + OracleContractReadProvider + 'static,
{
    pub(in super::super) fn sync_pending_queue<B: CacheRead>(&self, snapshot: &DataCache<B>) {
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

    pub(in super::super) fn snapshot_cache(&self) -> StoreDataCache<R::Store> {
        let snapshot = self.runtime.store().snapshot();
        let store_cache = StoreCache::<R::Store>::new_from_snapshot(snapshot);
        store_cache.data_cache().clone()
    }
}
