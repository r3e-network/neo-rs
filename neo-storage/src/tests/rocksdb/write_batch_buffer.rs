use super::*;
use rocksdb::WriteBatchIterator;
use tempfile::TempDir;

fn create_test_db() -> (Arc<DB>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test_db");

    let mut opts = rocksdb::Options::default();
    opts.create_if_missing(true);

    let db = Arc::new(DB::open(&opts, &path).unwrap());
    (db, tmp)
}

#[test]
fn write_batch_buffer_put_and_flush() {
    let (db, _tmp) = create_test_db();
    let buffer = WriteBatchBuffer::with_defaults(db);

    buffer.put(b"key1", b"value1");
    buffer.put(b"key2", b"value2");

    assert_eq!(buffer.pending_count(), 2);

    buffer.flush().unwrap();

    assert_eq!(buffer.pending_count(), 0);

    let stats = buffer.stats_snapshot();
    assert_eq!(stats.batches_flushed, 1);
    assert_eq!(stats.operations_written, 2);
}

#[test]
fn write_batch_buffer_delete() {
    let (db, _tmp) = create_test_db();
    let buffer = WriteBatchBuffer::with_defaults(db.clone());

    // First put a value
    buffer.put(b"key1", b"value1");
    buffer.flush().unwrap();

    // Then delete it
    buffer.delete(b"key1");
    buffer.flush().unwrap();

    // Verify it's gone
    let result = db.get(b"key1").unwrap();
    assert!(result.is_none());
}

#[test]
fn write_batch_buffer_extend_buffers_multiple_operations_with_one_flush_check() {
    let (db, _tmp) = create_test_db();
    let config = WriteBatchConfig {
        max_batch_size: 10,
        max_delay_ms: 10000,
        min_operations: 10,
        max_batch_bytes: 1024 * 1024,
        sync_on_flush: false,
        disable_wal: true,
    };
    let buffer = WriteBatchBuffer::new(db.clone(), config);

    buffer.extend([
        (b"delete-me".as_slice(), Some(b"before".as_slice())),
        (b"delete-me".as_slice(), None),
        (b"keep".as_slice(), Some(b"after".as_slice())),
    ]);

    let stats_before_flush = buffer.stats_snapshot();
    assert_eq!(stats_before_flush.pending_operations, 3);
    assert_eq!(stats_before_flush.batches_flushed, 0);

    buffer.flush().expect("flush bulk buffer");

    assert_eq!(db.get(b"delete-me").unwrap(), None);
    assert_eq!(
        db.get(b"keep").unwrap().as_deref(),
        Some(b"after".as_slice())
    );
    let stats_after_flush = buffer.stats_snapshot();
    assert_eq!(stats_after_flush.batches_flushed, 1);
    assert_eq!(stats_after_flush.operations_written, 3);
    assert_eq!(stats_after_flush.pending_operations, 0);
}

#[test]
fn write_batch_buffer_extend_from_visits_operations_with_one_flush_check() {
    let (db, _tmp) = create_test_db();
    let config = WriteBatchConfig {
        max_batch_size: 10,
        max_delay_ms: 10000,
        min_operations: 10,
        max_batch_bytes: 1024 * 1024,
        sync_on_flush: false,
        disable_wal: true,
    };
    let buffer = WriteBatchBuffer::new(db.clone(), config);

    buffer.extend_from(|sink| {
        sink(b"delete-me", Some(b"before"));
        sink(b"delete-me", None);
        sink(b"keep", Some(b"after"));
    });

    let stats_before_flush = buffer.stats_snapshot();
    assert_eq!(stats_before_flush.pending_operations, 3);
    assert_eq!(stats_before_flush.batches_flushed, 0);

    buffer.flush().expect("flush bulk buffer");

    assert_eq!(db.get(b"delete-me").unwrap(), None);
    assert_eq!(
        db.get(b"keep").unwrap().as_deref(),
        Some(b"after".as_slice())
    );
    let stats_after_flush = buffer.stats_snapshot();
    assert_eq!(stats_after_flush.batches_flushed, 1);
    assert_eq!(stats_after_flush.operations_written, 3);
    assert_eq!(stats_after_flush.pending_operations, 0);
}

#[test]
fn write_batch_buffer_set_config_flushes_pending_before_switching_modes() {
    let (db, _tmp) = create_test_db();
    let durable = WriteBatchConfig::durable();
    let fast = WriteBatchConfig::high_throughput();
    let buffer = WriteBatchBuffer::new(db.clone(), durable);

    buffer.put(b"before-mode-switch", b"durable");
    assert_eq!(buffer.pending_count(), 1);

    buffer.set_config(fast).expect("flush before config switch");

    assert_eq!(buffer.config(), fast);
    assert_eq!(buffer.pending_count(), 0);
    assert_eq!(
        db.get(b"before-mode-switch").unwrap().as_deref(),
        Some(b"durable".as_slice()),
        "writes queued under the old mode should flush before switching to fast-sync options"
    );
    assert_eq!(buffer.stats_snapshot().operations_written, 1);

    buffer.put(b"after-mode-switch", b"fast");
    assert_eq!(buffer.pending_count(), 1);
    buffer.flush().expect("flush after config switch");
    assert_eq!(
        db.get(b"after-mode-switch").unwrap().as_deref(),
        Some(b"fast".as_slice())
    );
}

#[test]
fn write_batch_buffer_flush_releases_pending_lock_before_rocksdb_write() {
    let source = include_str!("../../rocksdb/write_batch_buffer.rs");
    let flush_body = source
        .split("pub fn flush(&self) -> StorageResult<()>")
        .nth(1)
        .and_then(|tail| tail.split("fn restore_failed_flush_batch").next())
        .expect("flush body source");

    let detach_scope_end = flush_body
        .find("};")
        .expect("pending detach scope should end before db write");
    let write_call = flush_body
        .find("self.db.write_opt")
        .expect("flush should write detached batch to RocksDB");

    assert!(
        flush_body.contains("_flush_guard = self.flush_gate.lock()"),
        "concurrent flushes must stay serialized so older detached batches reach RocksDB before newer batches"
    );
    assert!(
        detach_scope_end < write_call,
        "flush should detach the pending batch and release the producer mutex before RocksDB I/O"
    );
}

#[test]
fn write_batch_replay_preserves_failed_then_newer_operation_order() {
    let mut failed = WriteBatch::default();
    failed.put(b"k1", b"failed");

    let mut newer = WriteBatch::default();
    newer.put(b"k1", b"newer");
    newer.delete(b"k2");

    let mut combined = WriteBatch::from_data(failed.data());
    newer.iterate(&mut ReplayIntoBatch {
        target: &mut combined,
    });

    let mut observed = Vec::new();
    combined.iterate(&mut RecordingBatchIterator(&mut observed));

    assert_eq!(
        observed,
        vec![
            ("put".to_string(), b"k1".to_vec(), Some(b"failed".to_vec())),
            ("put".to_string(), b"k1".to_vec(), Some(b"newer".to_vec())),
            ("delete".to_string(), b"k2".to_vec(), None),
        ],
        "failed flush recovery should retry the failed batch before newer pending writes"
    );
}

#[test]
fn write_batch_buffer_auto_flush_on_size() {
    let (db, _tmp) = create_test_db();
    let config = WriteBatchConfig {
        max_batch_size: 5,
        max_delay_ms: 10000, // Long delay to ensure size triggers flush
        min_operations: 10,
        max_batch_bytes: 1024 * 1024,
        sync_on_flush: false,
        disable_wal: true,
    };

    let buffer = WriteBatchBuffer::new(db, config);

    // Add 4 items - should not flush yet
    for i in 0..4 {
        buffer.put(format!("key{}", i).as_bytes(), b"value");
    }

    assert_eq!(buffer.pending_count(), 4);

    // Add 5th item - should trigger auto-flush
    buffer.put(b"key5", b"value");

    // May need a small delay for the flush to complete
    std::thread::sleep(Duration::from_millis(10));

    // Should be flushed or very close to it
    assert!(buffer.pending_count() < 5);
}

struct RecordingBatchIterator<'a>(&'a mut Vec<(String, Vec<u8>, Option<Vec<u8>>)>);

impl WriteBatchIterator for RecordingBatchIterator<'_> {
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.0
            .push(("put".to_string(), key.into_vec(), Some(value.into_vec())));
    }

    fn delete(&mut self, key: Box<[u8]>) {
        self.0.push(("delete".to_string(), key.into_vec(), None));
    }
}

#[test]
fn write_batch_buffer_clear() {
    let (db, _tmp) = create_test_db();
    let buffer = WriteBatchBuffer::with_defaults(db);

    buffer.put(b"key1", b"value1");
    buffer.put(b"key2", b"value2");

    assert_eq!(buffer.pending_count(), 2);

    buffer.clear();

    assert_eq!(buffer.pending_count(), 0);
}

#[test]
fn write_batch_stats_snapshot() {
    let stats = WriteBatchStats::new();

    stats.record_flush(10, 1000, 5);
    stats.record_flush(20, 2000, 10);

    let snapshot = stats.snapshot();

    assert_eq!(snapshot.batches_flushed, 2);
    assert_eq!(snapshot.operations_written, 30);
    assert_eq!(snapshot.bytes_written, 3000);
    assert_eq!(snapshot.total_flush_duration_ms, 15);

    assert_eq!(snapshot.avg_ops_per_flush(), 15.0);
    assert_eq!(snapshot.avg_bytes_per_flush(), 1500.0);
    assert_eq!(snapshot.avg_flush_duration_ms(), 7.5);
}

#[test]
fn write_batch_config_presets() {
    let high_throughput = WriteBatchConfig::high_throughput();
    assert!(
        high_throughput.max_batch_size >= 50_000,
        "fast-sync MPT/state overlays can reach about 1k entries per block; \
         keep the operation threshold high enough to batch many blocks"
    );
    assert!(
        high_throughput.max_batch_bytes >= 64 * 1024 * 1024,
        "fast-sync should batch large state-service overlays before a \
         foreground RocksDB flush"
    );
    assert!(high_throughput.disable_wal);

    let durable = WriteBatchConfig::durable();
    assert_eq!(durable.max_batch_size, 100);
    assert!(durable.sync_on_flush);
    assert!(!durable.disable_wal);

    let balanced = WriteBatchConfig::balanced();
    assert_eq!(balanced.max_batch_size, 500);
}
