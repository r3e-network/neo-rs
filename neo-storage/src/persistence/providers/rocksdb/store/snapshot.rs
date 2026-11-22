use super::*;
/// Overlay snapshot over a `RocksDbStore`: buffered writes staged in memory,
/// reads served from pending changes and a DB point-in-time, and an
/// all-or-nothing `try_commit`.
///
/// # Drop order invariant (R08)
///
/// `snapshot` MUST be released before `db` (the `Arc<DB>`) is freed.
/// `DbSnapshot::drop` calls `release_snapshot` on its parent `DB`; if the
/// `Arc` were released first and this were the last owner, the database would
/// be freed and the subsequent `release_snapshot` would be a use-after-free.
///
/// `snapshot` is wrapped in `ManuallyDrop` and released explicitly in the
/// `Drop` impl below, which runs before Rust auto-drops any other field.
/// This makes the invariant self-documenting and independent of field
/// declaration order.
#[allow(dead_code)] // Field order is intentional; see drop-order invariant above.
pub struct RocksDbSnapshot {
    /// Wrapped in `ManuallyDrop` so the explicit `Drop` impl can release it
    /// before `db` is freed. Do not reorder or unwrap without updating `Drop`.
    snapshot: mem::ManuallyDrop<DbSnapshot<'static>>,
    pending_changes: Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    has_pending_changes: AtomicBool,
    /// Optional read cache for this snapshot
    read_cache: Option<Arc<StorageReadCache>>,
    /// Read-ahead configuration
    read_ahead_config: ReadAheadConfig,
    db: Arc<DB>,
    store: Arc<RocksDbStore>,
    pub(super) store_view: Arc<dyn Store>,
    commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
}

impl RocksDbSnapshot {
    pub(super) fn new(
        db: Arc<DB>,
        store: Arc<RocksDbStore>,
        read_cache: Option<Arc<StorageReadCache>>,
        read_ahead_config: ReadAheadConfig,
    ) -> Self {
        let snapshot = Self::create_snapshot(&db);

        Self {
            snapshot: mem::ManuallyDrop::new(snapshot),
            pending_changes: Mutex::new(HashMap::new()),
            has_pending_changes: AtomicBool::new(false),
            read_cache,
            read_ahead_config,
            db,
            store_view: store.clone(),
            store,
            commit_profiler: None,
        }
    }

    fn create_snapshot(db: &Arc<DB>) -> DbSnapshot<'static> {
        // Create a snapshot using a `'static` DB reference while keeping the Arc alive.
        let db_ptr = Arc::into_raw(db.clone());
        let snapshot = unsafe {
            // SAFETY: `db_ptr` comes from an Arc clone that stays alive for this scope.
            // The `RocksDbSnapshot` struct also owns an `Arc<DB>`, so the DB outlives
            // the snapshot. We immediately balance the raw Arc below.
            let static_db: &'static DB = &*db_ptr;
            static_db.snapshot()
        };
        // Balance Arc::into_raw to avoid leaking the temporary clone.
        unsafe {
            Arc::from_raw(db_ptr);
        }
        snapshot
    }

    fn read_options(&self) -> ReadOptions {
        provider::build_read_options(Some(&*self.snapshot), &self.read_ahead_config)
    }

    fn point_read_options(&self) -> ReadOptions {
        let mut options = ReadOptions::default();
        options.set_snapshot(&*self.snapshot);
        options.set_verify_checksums(self.read_ahead_config.verify_checksums);
        options.fill_cache(self.read_ahead_config.fill_cache);
        options
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }

    fn pending_change(&self, key: &[u8]) -> Option<Option<Vec<u8>>> {
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return None;
        }
        self.pending_changes.lock().get(key).cloned()
    }

    fn merged_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return None;
        }
        let prefix = key_prefix.unwrap_or(&[]);
        let pending_changes = {
            let pending = self.pending_changes.lock();
            if pending.is_empty() {
                return None;
            }
            pending
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Vec<_>>()
        };

        let mut items = BTreeMap::new();
        let iterator = provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            prefix,
            SeekDirection::Forward,
            &self.read_ahead_config,
        );
        for res in iterator {
            let (key, value) = match res {
                Ok(entry) => entry,
                // R11: an iterator error means the merged view is incomplete.
                // Silently skipping the row would let callers treat absent
                // data as authoritative, so the scan fails closed instead.
                Err(err) => fail_fast_read("snapshot iterator", &err),
            };
            let key_vec = key.to_vec();
            if !prefix.is_empty() && !key_vec.starts_with(prefix) {
                break;
            }
            items.insert(key_vec, value.to_vec());
        }

        for (key, value) in pending_changes {
            if let Some(value) = value {
                items.insert(key, value);
            } else {
                items.remove(&key);
            }
        }

        let mut entries: Vec<_> = items.into_iter().collect();
        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        Some(entries)
    }

    fn commit_pending(
        &mut self,
        force_durable: bool,
    ) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.pending_changes.get_mut().is_empty() {
            self.has_pending_changes.store(false, Ordering::Release);
            return Ok(());
        }

        let mut write_opts = WriteOptions::default();
        let mut wal_disabled = false;
        if force_durable {
            write_opts.set_sync(true);
        } else {
            let batch_config = self.store.batch_config.read();
            write_opts.set_sync(batch_config.sync_on_flush);
            if batch_config.disable_wal {
                write_opts.disable_wal(true);
                wal_disabled = true;
            }
        }

        let batch = {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::WriteBatchBuild,
            );
            let mut batch = WriteBatch::default();
            for (key, value) in self.pending_changes.get_mut().iter() {
                match value {
                    Some(val) => batch.put(key, val),
                    None => batch.delete(key),
                }
            }
            batch
        };

        let write_result = {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::DatabaseWrite,
            );
            self.db.write_opt(batch, &write_opts)
        };
        if let Err(err) = write_result {
            error!(target: "neo", error = %err, durable = force_durable, "rocksdb snapshot commit failed");
            return Err(StorageError::CommitFailed(format!(
                "RocksDB write failed: {err}"
            )));
        }
        if wal_disabled {
            // No WAL record exists for this commit, so it only becomes durable
            // once the memtable is flushed.
            self.store
                .memtable_only_writes
                .store(true, Ordering::Release);
        }

        // Invalidate only after the atomic database write succeeds. Invalidating
        // before the write lets a concurrent latest-state read repopulate the
        // cache with the old value, which would then survive the commit. The
        // generation bump also prevents an in-flight read that already fetched
        // the pre-commit value from publishing it afterwards.
        {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::CacheInvalidation,
            );
            self.store.invalidate_read_cache(
                self.pending_changes
                    .get_mut()
                    .keys()
                    .map(|key| StorageKey::from_bytes(key))
                    .collect::<Vec<_>>(),
            );
        }
        self.pending_changes.get_mut().clear();
        self.has_pending_changes.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for RocksDbSnapshot {
    fn drop(&mut self) {
        // SAFETY: Drop the RocksDB snapshot before releasing the Arc<DB>.
        // `snapshot` holds a `'static` borrow of the DB; `DbSnapshot::drop`
        // calls `release_snapshot` on the underlying rocksdb::DB. If `db`
        // (the Arc) were freed first and this were the last owner, the DB
        // allocation would be gone and `release_snapshot` would access freed
        // memory (use-after-free, R08). ManuallyDrop lets us call
        // `drop_in_place` here, before Rust auto-drops `db` and `store`.
        unsafe { mem::ManuallyDrop::drop(&mut self.snapshot) };
        // `db` and `store` are dropped subsequently in declaration order.
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        if let Some(change) = self.pending_change(key.as_slice()) {
            return change;
        }

        match self.db.get_opt(key, &self.point_read_options()) {
            Ok(value) => value,
            Err(err) => fail_fast_read("snapshot get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if let Some(items) = self.merged_entries(key_prefix.map(Vec::as_slice), direction) {
            return Box::new(items.into_iter());
        }

        if direction == SeekDirection::Backward
            && let Some(prefix) = key_prefix
        {
            let iterator = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                Some(self.read_options()),
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iterator.filter_map(|res| match res {
                Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                Err(err) => fail_fast_read("snapshot iterator", &err),
            }));
        }

        let start = key_prefix.map(Vec::as_slice).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        let prefix = key_prefix.cloned();
        let iterator = iterator.take_while(move |res| match res {
            Ok((key, _)) => prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iterator.filter_map(|res| match res {
            Ok((k, v)) => Some((k.to_vec(), v.to_vec())),
            Err(err) => fail_fast_read("snapshot iterator", &err),
        }))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        if let Some(change) = self.pending_change(raw.as_slice()) {
            // R10: uncommitted values must never enter the shared
            // latest-state cache.
            return change.map(StorageItem::from_bytes);
        }

        // R10: a snapshot reads its own historical view. It must not consult
        // the shared cache (which holds latest-state values for keys mutated
        // after this snapshot was created) nor populate it with historical
        // values that post-commit readers would then observe.
        match self.db.get_opt(&raw, &self.point_read_options()) {
            Ok(Some(bytes)) => Some(StorageItem::from_bytes(bytes)),
            Ok(None) => None,
            Err(err) => fail_fast_read("snapshot get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(StorageKey::to_array);

        if let Some(items) = self.merged_entries(prefix_bytes.as_deref(), direction) {
            // R10: snapshot views must not populate the shared latest-state
            // read cache.
            return Box::new(items.into_iter().map(|(key_vec, value)| {
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value);
                (storage_key, storage_item)
            }));
        }

        if direction == SeekDirection::Backward
            && let Some(prefix) = prefix_bytes.as_ref()
        {
            let iter = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                Some(self.read_options()),
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iter.map(move |res| {
                let (key, value) = match res {
                    Ok(entry) => entry,
                    Err(err) => fail_fast_read("snapshot iterator", &err),
                };
                let key_vec: Vec<u8> = key.into();
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value.into());
                (storage_key, storage_item)
            }));
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);
        let prefix_boundary = prefix_bytes.clone();
        let iter = iter.take_while(move |res| match res {
            Ok((key, _)) => prefix_boundary
                .as_ref()
                .is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => fail_fast_read("snapshot iterator", &err),
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes
                && !key_vec.starts_with(prefix)
            {
                return None;
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            let storage_item = StorageItem::from_bytes(value.into());
            Some((storage_key, storage_item))
        }))
    }
}

impl ReadOnlyStore for RocksDbSnapshot {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.pending_changes.get_mut().insert(key, None);
        self.has_pending_changes.store(true, Ordering::Release);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.pending_changes.get_mut().insert(key, Some(value));
        self.has_pending_changes.store(true, Ordering::Release);
        Ok(())
    }
}

impl StoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> Arc<dyn Store> {
        self.store_view.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        self.commit_pending(false)
    }

    fn set_commit_profiler(&mut self, profiler: Option<Arc<dyn StoreCommitProfiler>>) {
        self.commit_profiler = profiler;
    }

    fn try_commit_durable(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        self.commit_pending(true)
    }
}
