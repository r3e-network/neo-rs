use crate::{
    error::{StorageError, StorageResult},
    persistence::{
        commit_profile::{StoreCommitProfiler, StoreCommitStage, StoreCommitTimer},
        read_cache::{ReadCacheConfig, StorageReadCache},
        read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
        seek_direction::SeekDirection,
        storage::StorageConfig,
        store::{OnNewSnapshotDelegate, Store, StoreAccelerationStats, StoreCacheStats},
        store_snapshot::StoreSnapshot,
        write_batch_buffer::{WriteBatchConfig, WriteBatchStatsSnapshot},
        write_store::WriteStore,
    },
    types::{StorageItem, StorageKey},
};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    DB, DBIteratorWithThreadMode, ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs, mem,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tracing::{debug, error, warn};

use super::provider::{self, BatchCommitter, ReadAheadConfig};

mod admin;
mod read_only;
mod snapshot;
#[cfg(test)]
mod tests;

pub use snapshot::RocksDbSnapshot;

/// RocksDB-backed byte store implementing the workspace [`Store`]/[`WriteStore`]
/// traits, with write buffering, an optional point read cache, and
/// crash-safe snapshot commits.
pub struct RocksDbStore {
    pub(crate) db: Arc<DB>,
    pub(crate) on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
    pub(crate) batch_committer: Arc<BatchCommitter>,
    pub(crate) batch_config: RwLock<WriteBatchConfig>,
    pub(crate) read_cache: Option<Arc<StorageReadCache>>,
    pub(crate) read_ahead_config: ReadAheadConfig,
    /// Handles to the block and row caches this database runs with, retained
    /// purely so cache statistics can be read back.
    pub(crate) caches: provider::CacheHandles,
    /// Commit version of the database contents. Every write path advances it so
    /// readers can tell whether a value they just read predates a commit.
    cache_generation: Arc<AtomicU64>,
    /// Serializes cache invalidation against cache fills: a fill may only
    /// publish a value whose observed generation is still current.
    cache_gate: Arc<RwLock<()>>,
    /// Set when a write was accepted with the WAL disabled. Such a write has no
    /// durable copy until its memtable is flushed, even after the batch
    /// configuration is restored to WAL-backed mode.
    memtable_only_writes: Arc<AtomicBool>,
}

impl RocksDbStore {
    pub(crate) fn open(
        config: &StorageConfig,
        batch_config: WriteBatchConfig,
        read_cache_config: &Option<ReadCacheConfig>,
        enable_bloom_filters: bool,
        enable_read_ahead: bool,
    ) -> Result<Self, rocksdb::Error> {
        if let Some(parent) = config.path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = fs::create_dir_all(parent)
        {
            warn!(
                target: "neo",
                path = %config.path.display(),
                error = %err,
                "failed to create RocksDB data directory"
            );
        }

        let (options, caches) =
            provider::build_db_options_with_caches(config, enable_bloom_filters);
        let db = if config.read_only {
            Arc::new(DB::open_for_read_only(&options, &config.path, false)?)
        } else {
            Arc::new(DB::open(&options, &config.path)?)
        };

        let batch_committer = Arc::new(BatchCommitter::new(Arc::clone(&db), batch_config));

        let read_cache = read_cache_config
            .as_ref()
            .map(|cfg| Arc::new(StorageReadCache::new(*cfg)));

        let read_ahead_config = ReadAheadConfig {
            enabled: enable_read_ahead,
            ..Default::default()
        };

        Ok(Self {
            db,
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
            batch_committer,
            batch_config: RwLock::new(batch_config),
            read_cache,
            read_ahead_config,
            caches,
            cache_generation: Arc::new(AtomicU64::new(0)),
            cache_gate: Arc::new(RwLock::new(())),
            memtable_only_writes: Arc::new(AtomicBool::new(false)),
        })
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

    /// Returns the commit generation that cache fills must match to be published.
    fn read_cache_generation(&self) -> u64 {
        self.cache_generation.load(Ordering::Acquire)
    }

    /// Publishes a value read from the database if no commit raced the read.
    fn fill_read_cache(&self, key: &StorageKey, item: &StorageItem, expected_generation: u64) {
        fill_read_cache_if_current(
            self.read_cache.as_deref(),
            &self.cache_gate,
            &self.cache_generation,
            expected_generation,
            key,
            item,
        );
    }

    /// Advances the commit generation and drops the superseded keys.
    ///
    /// The generation bump and the key removal happen under the write side of
    /// the invalidation gate, so no in-flight reader that observed an older
    /// generation can republish a pre-commit value after this returns.
    fn invalidate_read_cache(&self, keys: impl IntoIterator<Item = StorageKey>) {
        let _gate = self.cache_gate.write();
        self.cache_generation.fetch_add(1, Ordering::AcqRel);
        if let Some(cache) = &self.read_cache {
            for key in keys {
                cache.remove(&key);
            }
        }
    }

    /// Drops every cached read after a write path that did not report its keys.
    ///
    /// Batched commits write through the batch buffer, which knows nothing about
    /// this store's cache entries. Clearing them wholesale is the only sound
    /// option, and it advances the generation so racing fills are discarded too.
    fn invalidate_all_reads(&self) {
        let _gate = self.cache_gate.write();
        self.cache_generation.fetch_add(1, Ordering::AcqRel);
        if let Some(cache) = &self.read_cache {
            cache.clear();
        }
    }

    /// Creates the same raw snapshot while preserving a caller's callback identity.
    /// Compatibility facades pass themselves; all handlers and snapshot state stay here.
    pub fn snapshot_for(&self, callback_store: &dyn Store) -> Arc<dyn StoreSnapshot> {
        self.snapshot_with_store(callback_store, |store| store)
    }

    /// Wraps only the exposed store handle, not the raw snapshot or commit state.
    /// This keeps compatibility downcasts and callback identity in upper layers.
    pub fn snapshot_with_store<F>(
        &self,
        callback_store: &dyn Store,
        wrap_store: F,
    ) -> Arc<dyn StoreSnapshot>
    where
        F: FnOnce(Arc<RocksDbStore>) -> Arc<dyn Store>,
    {
        let store_arc = Arc::new(self.clone());
        let store_view = wrap_store(Arc::clone(&store_arc));
        let mut snapshot = RocksDbSnapshot::new(
            self.db.clone(),
            store_arc,
            self.read_cache.clone(),
            self.read_ahead_config,
        );
        snapshot.store_view = store_view;
        let snapshot = Arc::new(snapshot);

        let handlers = self.on_new_snapshot.read();
        for handler in handlers.iter() {
            handler(callback_store, snapshot.clone());
        }

        snapshot
    }
}

/// R09: a RocksDB read error is unrecoverable for a deterministic state
/// machine — interpreting it as "key absent" lets state decisions run on
/// fabricated data. Fail fast instead of returning `None`.
fn fail_fast_read(op: &str, err: &rocksdb::Error) -> ! {
    error!(target: "neo", error = %err, op, "rocksdb read failed; failing fast");
    panic!("RocksDB {op} failed: {err}");
}

/// Reads the byte count that follows `key` in a RocksDB property text blob.
///
/// `rocksdb.aggregated-table-properties` renders the table properties as
/// `# <name>=<value>` pairs, so the parser anchors on the key and then reads the
/// digits after the next `=`. Reading digits straight after the key would be
/// wrong: the index entry is named `index block size (user-key? N, delta-value?
/// N)` and would yield the flag inside the name instead of the size.
fn property_bytes(properties: &str, key: &str) -> Option<usize> {
    let after_key = &properties[properties.find(key)? + key.len()..];
    let value = &after_key[after_key.find('=')? + 1..];
    let digits: String = value
        .chars()
        .skip_while(|ch| ch.is_ascii_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Publishes a read-cache entry only when no invalidation happened since the
/// value was read from the database.
///
/// The generation check and the insertion happen under the read side of the
/// invalidation gate, while [`RocksDbStore::invalidate_read_cache`] advances the
/// generation and clears superseded keys under its write side. A reader that
/// observed an older generation therefore cannot re-insert a pre-commit value
/// after the invalidation ran.
fn fill_read_cache_if_current(
    read_cache: Option<&StorageReadCache>,
    gate: &RwLock<()>,
    generation: &AtomicU64,
    expected_generation: u64,
    key: &StorageKey,
    item: &StorageItem,
) {
    let Some(cache) = read_cache else {
        return;
    };
    let _gate = gate.read();
    if generation.load(Ordering::Acquire) != expected_generation {
        return;
    }
    let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
    cache.put(key.clone(), item.clone(), size);
}

impl RocksDbStore {
    /// Executes the existing direct delete through a shared backend handle.
    pub fn delete_raw(&self, key: Vec<u8>) -> StorageResult<()> {
        self.db.delete(&key).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb delete failed");
            StorageError::Io {
                message: format!("RocksDB delete failed: {err}"),
            }
        })?;
        self.invalidate_read_cache(std::iter::once(StorageKey::from_bytes(&key)));
        Ok(())
    }

    /// Executes the existing direct write without routing through batching.
    pub fn put_raw(&self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.db.put(&key, &value).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put failed");
            StorageError::Io {
                message: format!("RocksDB put failed: {err}"),
            }
        })?;
        self.invalidate_read_cache(std::iter::once(StorageKey::from_bytes(&key)));
        Ok(())
    }

    /// Executes the existing WAL-backed synchronous direct write.
    pub fn put_sync_raw(&self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db.put_opt(&key, &value, &options).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put_sync failed");
            StorageError::Io {
                message: format!("RocksDB put_sync failed: {err}"),
            }
        })?;
        self.invalidate_read_cache(std::iter::once(StorageKey::from_bytes(&key)));
        Ok(())
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.delete_raw(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.put_raw(key, value)
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.put_sync_raw(key, value)
    }
}

impl Store for RocksDbStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.snapshot_for(self)
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.on_new_snapshot.write().push(handler);
    }

    fn enable_fast_sync_mode(&self) {
        RocksDbStore::enable_fast_sync_mode(self);
    }

    fn disable_fast_sync_mode(&self) {
        RocksDbStore::disable_fast_sync_mode(self);
    }

    fn compact_all(&self) {
        if let Err(err) = RocksDbStore::compact_all(self) {
            warn!(target: "neo", error = %err, "manual full-range compaction failed");
        }
    }

    fn level_file_counts(&self) -> Vec<(u32, u64)> {
        RocksDbStore::level_file_counts(self)
    }

    fn flush(&self) {
        if let Err(err) = self.flush_batch_writes() {
            warn!(target: "neo", error = %err, "failed to flush pending RocksDB batch writes");
        }
        self.flush_memtables();
    }

    fn try_flush_durable(&self) -> StorageResult<()> {
        RocksDbStore::try_flush_durable(self)
    }

    fn supports_durable_writes(&self) -> bool {
        true
    }

    fn state_generation(&self) -> u64 {
        self.read_cache_generation()
    }

    fn cache_stats(&self) -> Option<StoreCacheStats> {
        Some(RocksDbStore::cache_stats(self))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn acceleration_stats(&self) -> Option<StoreAccelerationStats> {
        // The buffer object always exists, but it only produces telemetry once a
        // write path stages operations into it. The current snapshot commit path
        // calls `db.write_opt` directly, so an idle buffer is reported as absent
        // instead of as a component that is assumed to be working.
        let write_buffer = self.batch_committer.buffer.stats_snapshot();
        let write_buffer_active =
            write_buffer.batches_flushed > 0 || write_buffer.pending_operations > 0;
        Some(StoreAccelerationStats {
            read_cache: self.read_cache.as_ref().map(|cache| cache.stats()),
            write_buffer: write_buffer_active.then_some(write_buffer),
        })
    }
}

impl Clone for RocksDbStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
            batch_committer: Arc::clone(&self.batch_committer),
            batch_config: RwLock::new(*self.batch_config.read()),
            read_cache: self.read_cache.clone(),
            read_ahead_config: self.read_ahead_config,
            caches: self.caches.clone(),
            cache_generation: Arc::clone(&self.cache_generation),
            cache_gate: Arc::clone(&self.cache_gate),
            memtable_only_writes: Arc::clone(&self.memtable_only_writes),
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
