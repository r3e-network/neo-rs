#![allow(unsafe_code)]

use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    storage::StorageConfig,
    store::{RawOverlaySource, RocksDbBatchMetrics, Store, StoreBackendKind},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::rocksdb::write_batch_buffer::{WriteBatchConfig, WriteBatchStatsSnapshot};
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    DB, DBIteratorWithThreadMode, ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions,
};
use std::{
    collections::BTreeMap,
    fs, mem,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tracing::{debug, error, warn};

use super::find_iterator::{RocksDbRawFindIterator, RocksDbStorageFindIterator};
use super::provider::{self, BatchCommitter, ReadAheadConfig};

/// Persistent RocksDB implementation of the Neo storage traits.
pub struct RocksDbStore {
    pub(crate) db: Arc<DB>,
    pub(crate) batch_committer: Arc<BatchCommitter>,
    pub(crate) batch_config: RwLock<WriteBatchConfig>,
    pub(crate) fast_sync_buffering: Arc<AtomicBool>,
    pub(crate) pending_fast_sync_overlay: Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>,
    pub(crate) read_ahead_config: ReadAheadConfig,
}

impl std::fmt::Debug for RocksDbStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RocksDbStore").finish_non_exhaustive()
    }
}

impl RocksDbStore {
    pub(crate) fn open(
        config: &StorageConfig,
        batch_config: WriteBatchConfig,
        enable_bloom_filters: bool,
        enable_read_ahead: bool,
    ) -> Result<Self, rocksdb::Error> {
        if let Some(parent) = config.path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(parent) {
                    warn!(
                        target: "neo",
                        path = %config.path.display(),
                        error = %err,
                        "failed to create RocksDB data directory"
                    );
                }
            }
        }

        let options = provider::build_db_options(config, enable_bloom_filters);
        let db = if config.read_only {
            Arc::new(DB::open_for_read_only(&options, &config.path, false)?)
        } else {
            Arc::new(DB::open(&options, &config.path)?)
        };

        let batch_committer = Arc::new(BatchCommitter::new(Arc::clone(&db), batch_config));

        let read_ahead_config = ReadAheadConfig {
            enabled: enable_read_ahead,
            ..Default::default()
        };

        Ok(Self {
            db,
            batch_committer,
            batch_config: RwLock::new(batch_config),
            fast_sync_buffering: Arc::new(AtomicBool::new(false)),
            pending_fast_sync_overlay: Arc::new(RwLock::new(BTreeMap::new())),
            read_ahead_config,
        })
    }

    fn has_pending_fast_sync_overlay(&self) -> bool {
        !self.pending_fast_sync_overlay.read().is_empty()
    }

    fn pending_fast_sync_value(&self, key: &[u8]) -> Option<Option<Vec<u8>>> {
        self.pending_fast_sync_overlay.read().get(key).cloned()
    }

    fn record_pending_fast_sync_overlay(&self, entries: &[(Vec<u8>, Option<Vec<u8>>)]) {
        if entries.is_empty() {
            return;
        }
        let mut overlay = self.pending_fast_sync_overlay.write();
        for (key, value) in entries {
            overlay.insert(key.clone(), value.clone());
        }
    }

    fn clear_pending_fast_sync_overlay(&self) {
        self.pending_fast_sync_overlay.write().clear();
    }

    fn buffer_fast_sync_overlay_entries(&self, entries: &[(Vec<u8>, Option<Vec<u8>>)]) {
        if entries.is_empty() {
            return;
        }
        self.batch_committer.buffer.extend(
            entries
                .iter()
                .map(|(key, value)| (key.as_slice(), value.as_deref())),
        );
        self.record_pending_fast_sync_overlay(entries);
        if self.batch_committer.buffer.pending_count() == 0 {
            self.clear_pending_fast_sync_overlay();
        }
    }

    fn raw_key_matches_prefix(key: &[u8], prefix: Option<&[u8]>) -> bool {
        prefix.is_none_or(|prefix| key.starts_with(prefix))
    }

    fn collect_raw_db_entries(&self, prefix: Option<&[u8]>) -> Vec<(Vec<u8>, Vec<u8>)> {
        let start = prefix.unwrap_or(&[]);
        provider::iterator_from(
            self.db.as_ref(),
            None,
            start,
            SeekDirection::Forward,
            &self.read_ahead_config,
        )
        .filter_map(|res| match res {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!(target: "neo", error = %err, "rocksdb iterator error");
                None
            }
        })
        .take_while(|(key, _value)| Self::raw_key_matches_prefix(key.as_ref(), prefix))
        .map(|(key, value)| (key.to_vec(), value.to_vec()))
        .collect()
    }

    fn merge_pending_fast_sync_overlay(
        &self,
        prefix: Option<&[u8]>,
        direction: SeekDirection,
        db_entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut merged = db_entries.into_iter().collect::<BTreeMap<_, _>>();
        for (key, value) in self.pending_fast_sync_overlay.read().iter() {
            if !Self::raw_key_matches_prefix(key, prefix) {
                continue;
            }
            match value {
                Some(value) => {
                    merged.insert(key.clone(), value.clone());
                }
                None => {
                    merged.remove(key);
                }
            }
        }
        let mut entries = merged.into_iter().collect::<Vec<_>>();
        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        entries
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        provider::iterator_from(
            self.db.as_ref(),
            None,
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }

    /// Commits raw byte-key overlay entries directly to RocksDB.
    ///
    /// This is for callers that already own a complete write overlay and do not
    /// need a snapshot read view or rollbackable pending-change map. It uses the
    /// same write options as [`RocksDbSnapshot::try_commit`] but avoids the
    /// snapshot allocation and duplicate BTreeMap bookkeeping on large batches.
    pub fn commit_raw_overlay<'a, I>(&self, overlay: I) -> StorageResult<()>
    where
        I: IntoIterator<Item = (&'a [u8], Option<&'a [u8]>)>,
    {
        let mut entries = overlay.into_iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        if entries.is_empty() {
            return Ok(());
        }

        if self.fast_sync_buffering.load(Ordering::Relaxed) {
            let buffered_entries = entries
                .iter()
                .map(|(key, value)| ((*key).to_vec(), (*value).map(<[u8]>::to_vec)))
                .collect::<Vec<_>>();
            self.buffer_fast_sync_overlay_entries(&buffered_entries);
            return Ok(());
        }

        let mut batch = WriteBatch::default();
        for (key, value) in entries {
            match value {
                Some(value) => batch.put(key, value),
                None => batch.delete(key),
            }
        }

        let mut write_opts = WriteOptions::default();
        let batch_config = self.batch_config.read();
        write_opts.set_sync(batch_config.sync_on_flush);
        if batch_config.disable_wal {
            write_opts.disable_wal(true);
        }
        drop(batch_config);

        self.db.write_opt(batch, &write_opts).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb raw overlay commit failed");
            crate::StorageError::CommitFailed(format!("RocksDB raw overlay commit failed: {err}"))
        })
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    type FindIterator<'a> = RocksDbRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        if let Some(value) = self.pending_fast_sync_value(key) {
            return value;
        }
        match self.db.get(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "RocksDB get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let prefix_bytes = key_prefix.cloned();
        if self.has_pending_fast_sync_overlay() {
            let entries = self.merge_pending_fast_sync_overlay(
                prefix_bytes.as_deref(),
                direction,
                self.collect_raw_db_entries(prefix_bytes.as_deref()),
            );
            return RocksDbRawFindIterator::overlay(entries);
        }

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iterator = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    None,
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return RocksDbRawFindIterator::cursor(iterator, Some(prefix.clone()));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        // Stop as soon as the scan leaves the prefix range. Keys are stored
        // sorted, so prefix-matching keys are contiguous from the seek point.
        RocksDbRawFindIterator::cursor(iterator, prefix_bytes)
    }
}

impl RawReadOnlyStore for RocksDbStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(value) = self.pending_fast_sync_value(key) {
            return value;
        }
        match self.db.get(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "RocksDB get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
    type FindIterator<'a> = RocksDbStorageFindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        if let Some(value) = self.pending_fast_sync_value(&raw) {
            return value.map(StorageItem::from_bytes);
        }
        match self.db.get(raw) {
            Ok(value) => value.map(StorageItem::from_bytes),
            Err(err) => {
                error!(target: "neo", error = %err, "RocksDB get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        if self.has_pending_fast_sync_overlay() {
            let entries = self.merge_pending_fast_sync_overlay(
                prefix_bytes.as_deref(),
                direction,
                self.collect_raw_db_entries(prefix_bytes.as_deref()),
            );
            return RocksDbStorageFindIterator::new(RocksDbRawFindIterator::overlay(entries));
        }

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iter = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    None,
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return RocksDbStorageFindIterator::new(RocksDbRawFindIterator::cursor(
                    iter,
                    Some(prefix.clone()),
                ));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);

        RocksDbStorageFindIterator::new(RocksDbRawFindIterator::cursor(iter, prefix_bytes))
    }
}

impl ReadOnlyStore for RocksDbStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.db.delete(&key).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb delete failed");
            crate::StorageError::Io {
                message: format!("RocksDB delete failed: {}", err),
            }
        })?;
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.db.put(&key, &value).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put failed");
            crate::StorageError::Io {
                message: format!("RocksDB put failed: {}", err),
            }
        })?;
        Ok(())
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db.put_opt(&key, &value, &options).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put_sync failed");
            crate::StorageError::Io {
                message: format!("RocksDB put_sync failed: {}", err),
            }
        })?;
        Ok(())
    }
}

impl Store for RocksDbStore {
    type Snapshot = RocksDbSnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        let store_arc = Arc::new(self.clone());
        Arc::new(RocksDbSnapshot::new(
            self.db.clone(),
            store_arc,
            self.read_ahead_config,
        ))
    }

    fn flush(&self) -> StorageResult<()> {
        // Propagate batch-write failures so callers can react to durability loss.
        self.flush_batch_writes()?;
        self.flush_memtables()
    }

    fn backend_kind(&self) -> StoreBackendKind {
        StoreBackendKind::RocksDb
    }

    fn rocksdb_batch_metrics(&self) -> Option<RocksDbBatchMetrics> {
        let stats = self.batch_commit_stats();
        let config = self.write_batch_config();
        Some(RocksDbBatchMetrics {
            pending_operations: stats.pending_operations as u64,
            batches_flushed: stats.batches_flushed,
            operations_written: stats.operations_written,
            bytes_written: stats.bytes_written,
            flush_timeouts: stats.flush_timeouts,
            avg_ops_per_flush: stats.avg_ops_per_flush() as u64,
            avg_bytes_per_flush: stats.avg_bytes_per_flush() as u64,
            avg_flush_duration_ms: stats.avg_flush_duration_ms() as u64,
            max_batch_size: config.max_batch_size as u64,
            max_batch_bytes: config.max_batch_bytes as u64,
            disable_wal: config.disable_wal,
        })
    }

    fn supports_fast_sync_mode(&self) -> bool {
        true
    }

    fn enable_fast_sync_mode(&self) {
        RocksDbStore::enable_fast_sync_mode(self);
    }

    fn disable_fast_sync_mode(&self) {
        RocksDbStore::disable_fast_sync_mode(self);
    }

    fn discard_pending_fast_sync_writes(&self) {
        RocksDbStore::discard_pending_fast_sync_writes(self);
    }

    fn has_pending_fast_sync_writes(&self) -> bool {
        RocksDbStore::has_pending_fast_sync_writes(self)
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        self.commit_raw_overlay(
            overlay
                .iter()
                .map(|(key, value)| (key.as_slice(), value.as_deref())),
        )?;
        Ok(true)
    }

    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        if self.fast_sync_buffering.load(Ordering::Relaxed) {
            let mut entries = Vec::new();
            let mut sink = |key: &[u8], value: Option<&[u8]>| {
                entries.push((key.to_vec(), value.map(Vec::from)));
            };
            overlay.visit_raw_overlay(&mut sink);
            self.buffer_fast_sync_overlay_entries(&entries);
            return Ok(true);
        }

        let mut batch = WriteBatch::default();
        let mut has_entries = false;
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            has_entries = true;
            match value {
                Some(value) => batch.put(key, value),
                None => batch.delete(key),
            }
        };
        overlay.visit_raw_overlay(&mut sink);

        if !has_entries {
            return Ok(true);
        }

        let mut write_opts = WriteOptions::default();
        let batch_config = self.batch_config.read();
        write_opts.set_sync(batch_config.sync_on_flush);
        if batch_config.disable_wal {
            write_opts.disable_wal(true);
        }
        drop(batch_config);

        self.db.write_opt(batch, &write_opts).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb borrowed raw overlay commit failed");
            crate::StorageError::CommitFailed(format!(
                "RocksDB borrowed raw overlay commit failed: {err}"
            ))
        })?;
        Ok(true)
    }

    fn try_commit_durable_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        // Fast-sync batches may already have auto-flushed with WAL disabled.
        // They are visible to readers but are not durable until their memtable
        // is flushed, so every canonical fence in fast-sync mode must persist
        // that prefix before writing its own WAL-synchronous batch.
        if self.fast_sync_buffering.load(Ordering::Acquire) || self.has_pending_fast_sync_writes() {
            self.flush()?;
        }

        let mut batch = WriteBatch::default();
        let mut has_entries = false;
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            has_entries = true;
            match value {
                Some(value) => batch.put(key, value),
                None => batch.delete(key),
            }
        };
        overlay.visit_raw_overlay(&mut sink);

        if !has_entries {
            return Ok(true);
        }

        let mut write_options = WriteOptions::default();
        write_options.set_sync(true);
        self.db.write_opt(batch, &write_options).map_err(|error| {
            error!(target: "neo", error = %error, "rocksdb durable overlay commit failed");
            StorageError::commit_failed(format!("RocksDB durable overlay commit failed: {error}"))
        })?;
        Ok(true)
    }
}

impl Clone for RocksDbStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            batch_committer: Arc::clone(&self.batch_committer),
            batch_config: RwLock::new(*self.batch_config.read()),
            fast_sync_buffering: Arc::clone(&self.fast_sync_buffering),
            pending_fast_sync_overlay: Arc::clone(&self.pending_fast_sync_overlay),
            read_ahead_config: self.read_ahead_config,
        }
    }
}

impl Drop for RocksDbStore {
    fn drop(&mut self) {
        if let Err(err) = self.flush_batch_writes() {
            warn!(
                target: "neo",
                error = %err,
                "RocksDbStore: failed to flush pending batch writes during drop"
            );
        }
    }
}

/// Mutable point-in-time snapshot over a RocksDB store.
pub struct RocksDbSnapshot {
    // This field must be declared before the Arc<DB> fields so it is dropped
    // first. The RocksDB snapshot lifetime is widened to 'static in
    // create_snapshot while the DB Arcs below keep the database alive.
    snapshot: DbSnapshot<'static>,
    db: Arc<DB>,
    store: Arc<RocksDbStore>,
    write_batch: Mutex<WriteBatch>,
    pending_changes: Mutex<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Read-ahead configuration
    read_ahead_config: ReadAheadConfig,
}

impl std::fmt::Debug for RocksDbSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RocksDbSnapshot").finish_non_exhaustive()
    }
}

impl RocksDbSnapshot {
    fn new(db: Arc<DB>, store: Arc<RocksDbStore>, read_ahead_config: ReadAheadConfig) -> Self {
        let snapshot = Self::create_snapshot(&db);

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
            pending_changes: Mutex::new(BTreeMap::new()),
            read_ahead_config,
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
        provider::build_read_options(Some(&self.snapshot), &self.read_ahead_config)
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
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    type FindIterator<'a> = RocksDbRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self.db.get_opt(key, &self.read_options()) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "RocksDB snapshot get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let prefix_bytes = key_prefix.cloned();

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iterator = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    Some(self.read_options()),
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return RocksDbRawFindIterator::cursor(iterator, Some(prefix.clone()));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iterator = provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            start,
            direction,
            &self.read_ahead_config,
        );
        RocksDbRawFindIterator::cursor(iterator, prefix_bytes)
    }
}

impl RawReadOnlyStore for RocksDbSnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.db.get_opt(key, &self.read_options()) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "RocksDB snapshot get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    type FindIterator<'a> = RocksDbStorageFindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();

        self.db
            .get_opt(&raw, &self.read_options())
            .ok()
            .flatten()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iter = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    Some(self.read_options()),
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return RocksDbStorageFindIterator::new(RocksDbRawFindIterator::cursor(
                    iter,
                    Some(prefix.clone()),
                ));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);

        RocksDbStorageFindIterator::new(RocksDbRawFindIterator::cursor(iter, prefix_bytes))
    }
}

impl ReadOnlyStore for RocksDbSnapshot {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.write_batch.lock().delete(key.clone());
        self.pending_changes.lock().insert(key, None);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.write_batch.lock().put(key.clone(), value.clone());
        self.pending_changes.lock().insert(key, Some(value));
        Ok(())
    }
}

impl StoreSnapshot for RocksDbSnapshot {
    type Store = RocksDbStore;

    fn store(&self) -> Arc<Self::Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        use crate::StorageError;

        let mut batch_guard = self.write_batch.lock();
        let mut pending_guard = self.pending_changes.lock();

        if batch_guard.is_empty() {
            return Ok(());
        }

        let batch_data = batch_guard.data().to_vec();
        let pending_snapshot = pending_guard.clone();

        let mut batch = WriteBatch::default();
        mem::swap(&mut *batch_guard, &mut batch);
        pending_guard.clear();
        drop(pending_guard);
        drop(batch_guard);

        let _start = Instant::now();

        let mut write_opts = WriteOptions::default();
        let batch_config = self.store.batch_config.read();
        write_opts.set_sync(batch_config.sync_on_flush);
        if batch_config.disable_wal {
            write_opts.disable_wal(true);
        }
        drop(batch_config);

        if let Err(err) = self.db.write_opt(batch, &write_opts) {
            let mut batch_guard = self.write_batch.lock();
            let mut pending_guard = self.pending_changes.lock();
            *batch_guard = WriteBatch::from_data(&batch_data);
            *pending_guard = pending_snapshot;
            error!(target: "neo", error = %err, "rocksdb snapshot commit failed");
            return Err(StorageError::CommitFailed(format!(
                "RocksDB write failed: {}",
                err
            )));
        }

        Ok(())
    }
}

// These methods form the operational API for RocksDbStore (fast-sync, diagnostics).
// The struct is crate-private so the compiler flags them as dead code, but they are
// intentionally kept for use by higher-level subsystems.
// Rationale: backend-specific operational hooks are reached through concrete
// storage composition paths that are not visible to this module's lint pass.
#[allow(dead_code)]
impl RocksDbStore {
    /// Enables fast sync mode optimizations (disable WAL, reduce fsync).
    pub fn enable_fast_sync_mode(&self) {
        // Switch to high-throughput batch config (disable WAL, no fsync)
        let config = WriteBatchConfig::high_throughput();
        if let Err(err) = self.batch_committer.buffer.set_config(config) {
            warn!(
                target: "neo",
                error = %err,
                "failed to flush pending batch writes before enabling fast-sync mode"
            );
            return;
        }
        *self.batch_config.write() = config;
        self.fast_sync_buffering.store(true, Ordering::Relaxed);

        if let Err(err) = self.db.set_options(&[("disable_auto_compactions", "true")]) {
            warn!(target: "neo", error = %err, "failed to disable auto compactions");
        }
        debug!(target: "neo", "enabled fast sync mode optimizations (WAL disabled, auto compaction disabled)");
    }

    /// Disables fast sync mode optimizations.
    pub fn disable_fast_sync_mode(&self) {
        // Restore balanced batch config (WAL enabled)
        let config = WriteBatchConfig::balanced();
        if let Err(err) = self.flush_batch_writes() {
            warn!(
                target: "neo",
                error = %err,
                "failed to flush pending fast-sync batch writes before restoring WAL"
            );
        }
        if let Err(err) = self.batch_committer.buffer.set_config(config) {
            warn!(
                target: "neo",
                error = %err,
                "failed to switch batch writer to balanced mode"
            );
            return;
        }
        *self.batch_config.write() = config;
        self.fast_sync_buffering.store(false, Ordering::Relaxed);

        if let Err(err) = self
            .db
            .set_options(&[("disable_auto_compactions", "false")])
        {
            warn!(target: "neo", error = %err, "failed to enable auto compactions");
        }
        debug!(target: "neo", "disabled fast sync mode optimizations (WAL restored)");
    }

    /// Abandons buffered writes that were accepted into the fast-sync batch
    /// writer but never finalized. This is the failure-path counterpart to
    /// [`Self::flush_batch_writes`]: use it only before aborting a failed import.
    pub fn discard_pending_fast_sync_writes(&self) {
        self.batch_committer.buffer.clear();
        self.clear_pending_fast_sync_overlay();
    }

    /// Force flush all memtables to disk.
    pub fn flush_memtables(&self) -> StorageResult<()> {
        self.db.flush_wal(true).map_err(|error| {
            StorageError::commit_failed(format!("RocksDB WAL flush failed: {error}"))
        })?;
        self.db
            .flush_opt(&rocksdb::FlushOptions::default())
            .map_err(|error| {
                StorageError::commit_failed(format!("RocksDB memtable flush failed: {error}"))
            })
    }

    /// Returns memory usage statistics.
    pub fn memory_usage(&self) -> Option<(u64, u64)> {
        self.db
            .property_int_value("rocksdb.cur-size-active-mem-table")
            .ok()
            .flatten()
            .map(|active| {
                let total = self
                    .db
                    .property_int_value("rocksdb.cur-size-all-mem-tables")
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                (active, total)
            })
    }

    /// Returns batch commit statistics.
    pub fn batch_commit_stats(&self) -> WriteBatchStatsSnapshot {
        self.batch_committer.buffer.stats_snapshot()
    }

    /// Returns whether fast-sync writes have been accepted but are not yet
    /// guaranteed visible through RocksDB snapshots.
    pub fn has_pending_fast_sync_writes(&self) -> bool {
        self.has_pending_fast_sync_overlay() || self.batch_committer.buffer.has_pending()
    }

    /// Returns the active write-batch configuration.
    pub fn write_batch_config(&self) -> WriteBatchConfig {
        *self.batch_config.read()
    }

    /// Forces a flush of pending batch writes.
    pub fn flush_batch_writes(&self) -> StorageResult<()> {
        self.batch_committer.buffer.force_flush()?;
        self.clear_pending_fast_sync_overlay();
        Ok(())
    }
}
