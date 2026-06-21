use super::*;
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
    assert_eq!(high_throughput.max_batch_size, 5000);
    assert!(high_throughput.disable_wal);

    let durable = WriteBatchConfig::durable();
    assert_eq!(durable.max_batch_size, 100);
    assert!(durable.sync_on_flush);
    assert!(!durable.disable_wal);

    let balanced = WriteBatchConfig::balanced();
    assert_eq!(balanced.max_batch_size, 500);
}
