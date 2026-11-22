use super::*;

pub(super) struct EmptyBlockProfileGuard {
    previous: bool,
}

impl EmptyBlockProfileGuard {
    pub(super) fn new(empty: bool) -> Self {
        let previous = PROFILE_EMPTY_BLOCK.with(|current| current.replace(empty));
        Self { previous }
    }
}

impl Drop for EmptyBlockProfileGuard {
    fn drop(&mut self) {
        PROFILE_EMPTY_BLOCK.with(|current| current.set(self.previous));
    }
}

/// A flat write-back cache shared only by a bounded run of transaction-free blocks.
///
/// Every canonical block commit harvests just that block's delta and then uses
/// `PersistenceDataCache::commit` to retain visible values as hot `None` entries. No database
/// write occurs until `finish`, which applies the coalesced final state in one
/// physical transaction. Dropping this object before `finish` writes nothing.
pub(super) struct EmptyBlockWriteBack {
    transaction: Mutex<Option<StoreTransaction>>,
    pub(super) base_snapshot: Arc<PersistenceDataCache>,
    changes: Mutex<BTreeMap<neo_storage::StorageKey, Option<neo_storage::StorageItem>>>,
    committed_blocks: AtomicU64,
    expected_blocks: usize,
}

impl EmptyBlockWriteBack {
    pub(super) fn new(
        store: Arc<dyn Store>,
        commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
        expected_blocks: usize,
    ) -> Self {
        let config = DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            ..Default::default()
        };
        let mut transaction = StoreTransaction::from_snapshot_with_config(store.snapshot(), config);
        transaction.set_commit_profiler(commit_profiler);
        let base_snapshot = Arc::new(transaction.cache().data_cache().clone());
        Self {
            transaction: Mutex::new(Some(transaction)),
            base_snapshot,
            changes: Mutex::new(BTreeMap::new()),
            committed_blocks: AtomicU64::new(0),
            expected_blocks,
        }
    }

    pub(super) fn commit_logical_block(&self) -> Result<(), String> {
        let mut changes = self.changes.lock();
        self.base_snapshot
            .for_each_tracked_change(|key, item, state| {
                let value = match state {
                    neo_storage::persistence::TrackState::Added
                    | neo_storage::persistence::TrackState::Changed => Some(item.clone()),
                    neo_storage::persistence::TrackState::Deleted => None,
                    neo_storage::persistence::TrackState::None
                    | neo_storage::persistence::TrackState::NotFound => return,
                };
                changes.insert(key.clone(), value);
            });
        // Preserve all visible values as hot entries while clearing only the
        // tracked delta. The next block therefore sees its predecessor without
        // inheriting that predecessor's pending MPT/storage changes.
        self.base_snapshot.commit();
        if self.base_snapshot.has_pending_changes() {
            return Err("logical empty-block commit left pending changes".to_string());
        }
        self.committed_blocks.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<(), String> {
        let committed = self.committed_blocks.load(Ordering::Relaxed);
        if committed != self.expected_blocks as u64 {
            return Err(format!(
                "empty-block batch committed {committed} logical blocks; expected {}",
                self.expected_blocks
            ));
        }
        if self.base_snapshot.has_pending_changes() {
            return Err("empty-block batch finished with uncommitted logical changes".to_string());
        }

        let mut transaction = self
            .transaction
            .into_inner()
            .ok_or_else(|| "empty-block physical transaction was already consumed".to_string())?;
        for (key, value) in self.changes.into_inner() {
            match value {
                Some(item) => transaction
                    .cache_mut()
                    .try_update(key, item)
                    .map_err(|error| error.to_string())?,
                None => transaction
                    .cache_mut()
                    .try_delete(key)
                    .map_err(|error| error.to_string())?,
            }
        }
        transaction.commit().map_err(|error| error.to_string())
    }
}
