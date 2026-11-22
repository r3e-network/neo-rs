use super::*;
use neo_primitives::UnhandledExceptionPolicy;
use neo_storage::persistence::{
    SeekDirection,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    storage::StorageError,
    store::OnNewSnapshotDelegate,
    write_store::WriteStore,
};
use neo_storage::{StorageItem, StorageKey};

#[derive(Clone)]
struct FailingStore;

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingStore {
    fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&Vec<u8>>,
        _direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(std::iter::empty())
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
    fn try_get(&self, _key: &StorageKey) -> Option<StorageItem> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&StorageKey>,
        _direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        Box::new(std::iter::empty())
    }
}

impl ReadOnlyStore for FailingStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
    fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
        Ok(())
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
        Ok(())
    }
}

impl Store for FailingStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        Arc::new(FailingSnapshot {
            store: Arc::new(self.clone()),
        })
    }

    fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FailingSnapshot {
    store: Arc<dyn Store>,
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
    fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&Vec<u8>>,
        _direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(std::iter::empty())
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
    fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
        Ok(())
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
        Err(StorageError::invalid_operation(
            "injected application logs write failure",
        ))
    }
}

impl StoreSnapshot for FailingSnapshot {
    fn store(&self) -> Arc<dyn Store> {
        Arc::clone(&self.store)
    }

    fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
        Err(StorageError::CommitFailed(
            "injected application logs commit failure".to_string(),
        ))
    }
}

fn settings(exception_policy: UnhandledExceptionPolicy) -> ApplicationLogsSettings {
    ApplicationLogsSettings {
        exception_policy,
        ..ApplicationLogsSettings::default()
    }
}

fn memory_service() -> (
    ApplicationLogsService,
    Arc<neo_storage::persistence::providers::MemoryStore>,
) {
    let store = Arc::new(neo_storage::persistence::providers::MemoryStore::new());
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::clone(&store) as Arc<dyn Store>,
    );
    (service, store)
}

/// A logs store that supports the fallible durable flush interface.
///
/// `MemoryStore` inherits the trait default that rejects explicit durable
/// flushes, so a checkpoint cannot run against it. This wrapper keeps the
/// memory storage, adds the flush gate, records how many records the store
/// already held at each flush, and can be armed to fail.
struct DurableMemoryStore {
    inner: neo_storage::persistence::providers::MemoryStore,
    flush_fails: AtomicBool,
    flushes: Arc<Mutex<Vec<usize>>>,
}

impl DurableMemoryStore {
    fn new(flushes: Arc<Mutex<Vec<usize>>>) -> Self {
        Self {
            inner: neo_storage::persistence::providers::MemoryStore::new(),
            flush_fails: AtomicBool::new(false),
            flushes,
        }
    }

    fn fail_flushes(&self) {
        self.flush_fails.store(true, Ordering::SeqCst);
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for DurableMemoryStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for DurableMemoryStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStore for DurableMemoryStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for DurableMemoryStore {
    fn delete(&mut self, key: Vec<u8>) -> Result<(), StorageError> {
        self.inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StorageError> {
        self.inner.put(key, value)
    }
}

impl Store for DurableMemoryStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.inner.snapshot()
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.inner.on_new_snapshot(handler);
    }

    fn try_flush_durable(&self) -> neo_storage::StorageResult<()> {
        let stored_records = self
            .inner
            .find(None::<&Vec<u8>>, SeekDirection::Forward)
            .count();
        self.flushes.lock().push(stored_records);
        if self.flush_fails.load(Ordering::SeqCst) {
            return Err(StorageError::CommitFailed(
                "injected application logs flush failure".to_string(),
            ));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A logs service over a store that supports the fallible durable flush.
fn durable_service() -> (
    ApplicationLogsService,
    Arc<DurableMemoryStore>,
    Arc<Mutex<Vec<usize>>>,
) {
    let flushes = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::new(DurableMemoryStore::new(Arc::clone(&flushes)));
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::clone(&store) as Arc<dyn Store>,
    );
    (service, store, flushes)
}

fn block_log_hash(byte: u8) -> UInt256 {
    UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
}

fn stored_block_log(
    store: &neo_storage::persistence::providers::MemoryStore,
    hash: &UInt256,
) -> Option<Value> {
    let mut key = vec![ApplicationLogsService::PREFIX_BLOCK];
    key.extend_from_slice(&hash.to_bytes());
    let raw = store.try_get(&key)?;
    serde_json::from_slice(&raw).ok()
}

/// Writes one block's records the way the committing callback does,
/// including the per-block snapshot request that must be a no-op inside a
/// unit and the block boundary that makes a commit report a height.
fn commit_one_inside_unit(service: &ApplicationLogsService, hash: &UInt256, height: u32) {
    service.note_block_boundary(height, *hash);
    service.start_batch();
    service
        .write_log(
            ApplicationLogsService::PREFIX_BLOCK,
            hash,
            serde_json::json!({ "blockhash": hash.to_string() }),
        )
        .expect("stage application log");
    service
        .commit_batch()
        .expect("promote staged application log inside the unit");
}

/// Persists one block inside its own unit, exactly as the importer does.
fn finish_one_unit(service: &ApplicationLogsService, hash: &UInt256, height: u32) {
    assert_eq!(
        service.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    commit_one_inside_unit(service, hash, height);
    assert_eq!(
        service.finish_persistence_unit().expect("finish unit"),
        PersistenceUnitOutcome::Finished { records: 1 }
    );
}

#[test]
fn persistence_unit_abort_discards_records_staged_since_begin() {
    let (service, store) = memory_service();
    let hash = block_log_hash(0x11);

    assert_eq!(
        service.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    commit_one_inside_unit(&service, &hash, 1);
    assert_eq!(
        service.abort_persistence_unit(),
        PersistenceUnitOutcome::Aborted { records: 1 }
    );

    assert_eq!(service.flush_accepted().expect("flush").records, 0);
    assert!(
        stored_block_log(&store, &hash).is_none(),
        "an aborted unit must not leave any application log behind"
    );
}

#[test]
fn persistence_unit_finish_preserves_records_until_flush() {
    let (service, store) = memory_service();
    let hash = block_log_hash(0x22);

    assert_eq!(
        service.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    commit_one_inside_unit(&service, &hash, 1);
    assert!(
        stored_block_log(&store, &hash).is_none(),
        "a record staged inside a unit must not reach the store before flush"
    );

    assert_eq!(
        service.finish_persistence_unit().expect("finish unit"),
        PersistenceUnitOutcome::Finished { records: 1 }
    );
    assert!(
        stored_block_log(&store, &hash).is_none(),
        "finishing a unit only promotes records, it does not commit them"
    );

    assert_eq!(service.flush_accepted().expect("flush").records, 1);
    let stored = stored_block_log(&store, &hash).expect("committed block log");
    assert_eq!(
        stored["blockhash"].as_str(),
        Some(hash.to_string().as_str())
    );
    assert_eq!(
        service.flush_accepted().expect("second flush").records,
        0,
        "a flush must not resend records it already committed"
    );
}

#[test]
fn persistence_unit_abort_keeps_records_of_an_earlier_finished_unit() {
    let (service, store) = memory_service();
    let kept = block_log_hash(0x33);
    let dropped = block_log_hash(0x44);

    finish_one_unit(&service, &kept, 1);

    service.begin_persistence_unit();
    commit_one_inside_unit(&service, &dropped, 2);
    assert_eq!(
        service.abort_persistence_unit(),
        PersistenceUnitOutcome::Aborted { records: 1 }
    );

    assert_eq!(service.flush_accepted().expect("flush").records, 1);
    assert!(stored_block_log(&store, &kept).is_some());
    assert!(stored_block_log(&store, &dropped).is_none());
}

#[test]
fn persistence_unit_lifecycle_survives_misuse() {
    let (service, _store) = memory_service();
    let hash = block_log_hash(0x55);

    // finish/abort without an open unit are no-ops, not panics.
    assert_eq!(
        service.finish_persistence_unit().expect("finish unit"),
        PersistenceUnitOutcome::NotOpen
    );
    assert_eq!(
        service.abort_persistence_unit(),
        PersistenceUnitOutcome::NotOpen
    );

    // A second begin keeps the unit that is already open.
    assert_eq!(
        service.begin_persistence_unit(),
        PersistenceUnitOutcome::Started
    );
    commit_one_inside_unit(&service, &hash, 1);
    assert_eq!(
        service.begin_persistence_unit(),
        PersistenceUnitOutcome::AlreadyOpen
    );
    assert_eq!(
        service.finish_persistence_unit().expect("finish unit"),
        PersistenceUnitOutcome::Finished { records: 1 }
    );
    assert_eq!(service.flush_accepted().expect("flush").records, 1);

    // A double finish cannot re-promote anything.
    assert_eq!(
        service.finish_persistence_unit().expect("finish unit"),
        PersistenceUnitOutcome::NotOpen
    );
    assert_eq!(service.flush_accepted().expect("flush").records, 0);
}

#[test]
fn online_mode_keeps_per_block_snapshot_commit() {
    let (service, store) = memory_service();
    let hash = block_log_hash(0x66);

    // No unit is opened in online mode: the committing callback installs a
    // snapshot and the committed callback commits it, exactly as before.
    service.note_block_boundary(1, hash);
    service.start_batch();
    service
        .write_log(
            ApplicationLogsService::PREFIX_BLOCK,
            &hash,
            serde_json::json!({ "blockhash": hash.to_string() }),
        )
        .expect("write application log");
    assert!(stored_block_log(&store, &hash).is_none());

    service.commit_batch().expect("commit application log");
    assert!(stored_block_log(&store, &hash).is_some());
    assert_eq!(
        service.flush_accepted().expect("flush").records,
        0,
        "online mode never parks records in the accepted set"
    );
    assert!(
        !service.import_session_active(),
        "online mode never opens an import session"
    );
    assert_eq!(
        service.watermark(),
        None,
        "nothing is tracked for the online per-block path"
    );
    assert_eq!(service.flush_telemetry(), LogsFlushTelemetry::default());
}

#[test]
fn import_session_coalesces_many_blocks_into_one_commit() {
    let (service, store) = memory_service();
    assert_eq!(
        service.begin_import_session(),
        ImportSessionOutcome::Started
    );

    let hashes = (1..=10u8).map(block_log_hash).collect::<Vec<_>>();
    for (offset, hash) in hashes.iter().enumerate() {
        let height = u32::try_from(offset).expect("test height") + 1;
        finish_one_unit(&service, hash, height);
    }

    // Ten finished units park ten records and write nothing on their own.
    let pending = service.flush_telemetry();
    assert_eq!(pending.flushes, 0);
    assert_eq!(pending.records, 0);
    assert_eq!(pending.pending_records, 10);
    for hash in &hashes {
        assert!(stored_block_log(&store, hash).is_none());
    }

    let ended = service.end_import_session().expect("end session");
    match ended {
        ImportSessionOutcome::Ended { records, bytes } => {
            assert_eq!(
                records, 10,
                "the closing commit writes every pending record"
            );
            assert!(bytes > 0, "the closing commit reports the bytes it wrote");
        }
        other => panic!("unexpected session outcome: {other:?}"),
    }

    // One commit carried all ten records, so the ten blocks cost one write.
    let telemetry = service.flush_telemetry();
    assert_eq!(telemetry.flushes, 1);
    assert_eq!(telemetry.records, 10);
    assert!(telemetry.bytes > 0);
    assert_eq!(telemetry.pending_records, 0);
    assert!(!service.import_session_active());
    for hash in &hashes {
        assert!(stored_block_log(&store, hash).is_some());
    }
}

/// Dumps every record of a logs store, in key order.
fn dump_logs(store: &neo_storage::persistence::providers::MemoryStore) -> Vec<(Vec<u8>, Vec<u8>)> {
    store.find(None, SeekDirection::Forward).collect()
}

/// The same ten blocks must store identical JSON whether they are committed
/// one unit at a time or accumulated into one coalesced commit.
///
/// Coalescing only changes *when* records are written, never what is
/// written: both runs push the same key/value pairs into the same store.
#[test]
fn coalesced_commit_stores_the_same_json_as_per_block_commits() {
    let hashes = (1..=10u8).map(block_log_hash).collect::<Vec<_>>();

    // Per-block commits: a zero byte cap commits after every finished unit.
    let (per_block, per_block_store) = memory_service();
    per_block.set_accepted_byte_cap(0);
    per_block.begin_import_session();
    for (offset, hash) in hashes.iter().enumerate() {
        let height = u32::try_from(offset).expect("test height") + 1;
        finish_one_unit(&per_block, hash, height);
    }
    per_block.end_import_session().expect("end session");

    // Coalesced commits: one unit per block, one commit at the end.
    let (coalesced, coalesced_store) = memory_service();
    coalesced.begin_import_session();
    for (offset, hash) in hashes.iter().enumerate() {
        let height = u32::try_from(offset).expect("test height") + 1;
        finish_one_unit(&coalesced, hash, height);
    }
    coalesced.end_import_session().expect("end session");

    assert_eq!(
        per_block.flush_telemetry().flushes,
        u64::try_from(hashes.len()).expect("record count"),
        "the per-block run must commit once per block"
    );
    assert_eq!(
        coalesced.flush_telemetry().flushes,
        1,
        "the coalesced run must commit once for the whole run"
    );

    let per_block_records = dump_logs(&per_block_store);
    assert_eq!(per_block_records.len(), hashes.len());
    assert_eq!(
        per_block_records,
        dump_logs(&coalesced_store),
        "coalescing must not change the stored bytes"
    );
}

/// Serialized size of one block-log record, measured through a session that
/// commits exactly that record.
fn single_record_bytes() -> u64 {
    let (service, _store) = memory_service();
    service.begin_import_session();
    finish_one_unit(&service, &block_log_hash(0x01), 1);
    service.end_import_session().expect("end session");
    service.flush_telemetry().bytes
}

#[test]
fn accepted_byte_cap_forces_a_commit_mid_accumulation() {
    let record_bytes = single_record_bytes();
    assert!(record_bytes > 0);

    // A cap only two records fit in: the third promotion crosses it, so the
    // commit lands mid-accumulation instead of at the next checkpoint.
    let (service, store) = memory_service();
    service.set_accepted_byte_cap(record_bytes * 2 + 1);
    assert_eq!(
        service.begin_import_session(),
        ImportSessionOutcome::Started
    );

    let hashes = (1..=5u8).map(block_log_hash).collect::<Vec<_>>();
    for (offset, hash) in hashes.iter().enumerate() {
        let height = u32::try_from(offset).expect("test height") + 1;
        finish_one_unit(&service, hash, height);
    }

    let telemetry = service.flush_telemetry();
    assert_eq!(telemetry.flushes, 1, "the cap must commit on its own");
    assert_eq!(telemetry.records, 3, "the commit carries what crossed it");
    assert_eq!(telemetry.pending_records, 2);
    assert!(stored_block_log(&store, &hashes[0]).is_some());
    assert!(stored_block_log(&store, &hashes[2]).is_some());
    assert!(
        stored_block_log(&store, &hashes[3]).is_none(),
        "records after the cap commit stay pending"
    );

    service.end_import_session().expect("end session");
    let telemetry = service.flush_telemetry();
    assert_eq!(telemetry.flushes, 2);
    assert_eq!(telemetry.records, 5);
    assert_eq!(telemetry.pending_records, 0);

    // A cap of zero reproduces the per-block write pattern.
    let (per_block, _store) = memory_service();
    per_block.set_accepted_byte_cap(0);
    per_block.begin_import_session();
    for (offset, hash) in hashes.iter().take(3).enumerate() {
        let height = u32::try_from(offset).expect("test height") + 1;
        finish_one_unit(&per_block, hash, height);
    }
    assert_eq!(
        per_block.flush_telemetry().flushes,
        3,
        "a zero cap commits after every finished unit"
    );
}

#[test]
fn watermark_tracks_the_last_durably_flushed_height() {
    let (service, _store, _flushes) = durable_service();
    assert_eq!(service.watermark(), None);

    service.begin_import_session();
    assert_eq!(
        service.watermark(),
        None,
        "a new session starts without a watermark"
    );

    let first = block_log_hash(0x71);
    finish_one_unit(&service, &first, 5);
    assert_eq!(
        service.watermark(),
        None,
        "promoting a unit is not durability"
    );

    let flushed = service.flush_accepted().expect("flush");
    assert_eq!(flushed.records, 1);
    assert_eq!(
        flushed.watermark,
        Some((5, first)),
        "the commit reports the block its bytes cover"
    );
    assert_eq!(
        service.watermark(),
        None,
        "a commit alone must not certify durability"
    );

    service.try_flush_durable().expect("durable flush");
    assert_eq!(service.watermark(), Some((5, first)));

    let second = block_log_hash(0x72);
    finish_one_unit(&service, &second, 9);
    service.flush_accepted().expect("flush");
    service.try_flush_durable().expect("durable flush");
    assert_eq!(service.watermark(), Some((9, second)));

    // Ending the session keeps the watermark readable for resume validation.
    service.end_import_session().expect("end session");
    assert_eq!(service.watermark(), Some((9, second)));

    // The next session starts from nothing again.
    service.begin_import_session();
    assert_eq!(service.watermark(), None);
}

/// The watermark is the logs database's durable claim, so it may only move
/// after the backend confirmed the flush.
#[test]
fn durable_flush_advances_the_watermark_only_after_the_store_flush() {
    let (service, _store, flushes) = durable_service();
    service.begin_import_session();

    let hash = block_log_hash(0xC1);
    finish_one_unit(&service, &hash, 4);
    let flushed = service.flush_accepted().expect("commit");
    assert_eq!(flushed.records, 1);
    assert_eq!(service.watermark(), None, "the commit is not durability");

    service.try_flush_durable().expect("durable flush");
    assert_eq!(service.watermark(), Some((4, hash)));
    assert_eq!(service.flush_telemetry().watermark, Some((4, hash)));

    // The record was already in the store when the flush ran, so the commit
    // preceded it, and the watermark was still unset right before the call, so
    // its promotion can only have happened inside it.
    let observed = flushes.lock();
    assert_eq!(observed.len(), 1, "one commit must cost one durable flush");
    assert_eq!(observed[0], 1, "the record was committed before the flush");
}

#[test]
fn durable_flush_failure_keeps_the_watermark_unadvanced() {
    let (service, store, flushes) = durable_service();
    service.begin_import_session();

    let hash = block_log_hash(0xD1);
    finish_one_unit(&service, &hash, 6);
    assert_eq!(service.flush_accepted().expect("commit").records, 1);
    assert!(stored_block_log(&store.inner, &hash).is_some());
    assert_eq!(service.watermark(), None);

    store.fail_flushes();
    let error = service
        .try_flush_durable()
        .expect_err("a failing durable flush must surface its error");
    assert!(
        error.contains("injected application logs flush failure"),
        "unexpected error: {error}"
    );
    assert_eq!(
        service.watermark(),
        None,
        "a failed flush must not advance the watermark"
    );
    assert_eq!(
        service.flush_telemetry().watermark,
        None,
        "the telemetry must not report an undurable watermark either"
    );
    assert!(
        stored_block_log(&store.inner, &hash).is_some(),
        "a failed flush must not undo the records the commit wrote"
    );
    assert_eq!(*flushes.lock(), vec![1], "the failing flush still ran");
}

#[test]
fn import_session_lifecycle_survives_misuse() {
    let (service, store) = memory_service();

    assert_eq!(
        service.end_import_session().expect("end session"),
        ImportSessionOutcome::NotOpen,
        "ending without a session must not write anything"
    );
    assert!(!service.import_session_active());

    assert_eq!(
        service.begin_import_session(),
        ImportSessionOutcome::Started
    );
    assert_eq!(
        service.begin_import_session(),
        ImportSessionOutcome::AlreadyOpen,
        "a second begin keeps the open session"
    );

    let hash = block_log_hash(0x91);
    finish_one_unit(&service, &hash, 1);
    match service.end_import_session().expect("end session") {
        ImportSessionOutcome::Ended { records, bytes } => {
            assert_eq!(records, 1);
            assert!(bytes > 0);
        }
        other => panic!("unexpected session outcome: {other:?}"),
    }
    assert!(stored_block_log(&store, &hash).is_some());
    assert_eq!(
        service.end_import_session().expect("second end"),
        ImportSessionOutcome::NotOpen
    );
    assert!(!service.import_session_active());
}

#[test]
fn failed_commit_returns_the_records_to_the_pending_set() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::new(FailingStore),
    );
    service.begin_import_session();
    finish_one_unit(&service, &block_log_hash(0x81), 1);

    let error = service.flush_accepted().expect_err("write failure");
    assert!(error.contains("injected application logs write failure"));

    let telemetry = service.flush_telemetry();
    assert_eq!(telemetry.flushes, 0);
    assert_eq!(telemetry.records, 0);
    assert_eq!(
        telemetry.pending_records, 1,
        "a failed commit must keep its records pending"
    );
    assert_eq!(service.watermark(), None);
}

#[test]
fn automatic_cap_commit_failure_is_reported_and_keeps_records_pending() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::new(FailingStore),
    );
    service.set_accepted_byte_cap(0);
    service.begin_import_session();
    service.begin_persistence_unit();
    commit_one_inside_unit(&service, &block_log_hash(0xA1), 1);

    let error = service
        .finish_persistence_unit()
        .expect_err("the automatic cap commit must report its failure");
    assert!(error.contains("injected application logs write failure"));
    assert_eq!(service.flush_telemetry().pending_records, 1);
}

#[test]
fn application_logs_run_during_fast_sync_import() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::new(FailingStore),
    );
    assert!(CommittingHandler::run_during_fast_sync(&service));
}

#[test]
fn commit_batch_propagates_snapshot_try_commit_failure() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::new(FailingStore),
    );
    service.start_batch();

    let err = service
        .commit_batch()
        .expect_err("application logs commit should propagate snapshot commit failure");

    assert!(err.contains("injected application logs commit failure"));
}

#[test]
fn write_log_propagates_snapshot_put_failure() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::Ignore),
        Arc::new(FailingStore),
    );
    service.start_batch();

    let err = service
        .write_log(
            ApplicationLogsService::PREFIX_BLOCK,
            &UInt256::zero(),
            Value::Null,
        )
        .expect_err("application logs write should propagate snapshot put failure");

    assert!(err.contains("injected application logs write failure"));
}

#[test]
fn commit_error_disables_service_when_policy_stops_plugin() {
    let service = ApplicationLogsService::new(
        settings(UnhandledExceptionPolicy::StopPlugin),
        Arc::new(FailingStore),
    );

    service.handle_error("injected failure", "committed");

    assert!(service.disabled.load(Ordering::SeqCst));
}

#[test]
fn csharp_fault_exception_message_matches_addfee_text() {
    assert_eq!(
        csharp_fault_exception_message(
            "Gas exhausted: used 30, limit 1 [ip=0 opcode=PUSH1 eval_depth=0]"
        ),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("Gas exhausted: used 30, limit 1"),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("Insufficient gas: required 30, available 1 [ip=4]"),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("Gas limit exceeded"),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("Out of gas"),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("VM exceeded gas limit during execution"),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message("Insufficient GAS."),
        "Insufficient GAS."
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Called Contract Does Not Exist: 0xabc [ip=12 opcode=SYSCALL eval_depth=1]"
        ),
        "Called Contract Does Not Exist: 0xabc"
    );
    assert_eq!(
        csharp_fault_exception_message("user threw Gas exhausted: used 1, limit 1 extra"),
        "user threw Gas exhausted: used 1, limit 1 extra"
    );
    assert_eq!(
        csharp_fault_exception_message("keep [ip=not-a-diagnostic]"),
        "keep [ip=not-a-diagnostic]"
    );
    assert_eq!(
        csharp_fault_exception_message("ASSERT is executed with false result"),
        "ASSERT is executed with false result."
    );
    assert_eq!(
        csharp_fault_exception_message(
            "ASSERT is executed with false result [ip=8 opcode=ASSERT eval_depth=1]"
        ),
        "ASSERT is executed with false result."
    );
    assert_eq!(
        csharp_fault_exception_message("ASSERT is executed with false result: paused"),
        "ASSERT is executed with false result: paused"
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Interop service error: service System.Contract.Call, error: Invalid operation: Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb)."
        ),
        "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Interop service error: service System.Contract.Call, error: Invalid operation: Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb. [ip=20 opcode=SYSCALL eval_depth=2]"
        ),
        "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
        ),
        "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb). extra"
        ),
        "Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb). extra"
    );
    assert_eq!(
        csharp_fault_exception_message(
            "Interop service error: service System.Contract.Call, error: Invalid operation: something else"
        ),
        "Interop service error: service System.Contract.Call, error: Invalid operation: something else"
    );

    let mut value = serde_json::json!({
        "executions": [{
            "exception": "Gas exhausted: used 30, limit 1 [ip=0 opcode=PUSH1 eval_depth=0]"
        }]
    });
    assert!(normalize_stored_fault_exceptions(&mut value));
    assert_eq!(
        value["executions"][0]["exception"].as_str(),
        Some("Insufficient GAS.")
    );
    assert!(!normalize_stored_fault_exceptions(&mut value));
}
