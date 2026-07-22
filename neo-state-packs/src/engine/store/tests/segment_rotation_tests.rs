use super::*;
use std::fs;
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::Path;

const TEST_INDEX_MEMORY_BYTES: u64 = 16 * 1024 * 1024;

fn frame_bytes(epoch: u64, operations: &[PackOperation]) -> u64 {
    u64::try_from(encoded_test_frame(epoch, TEST_FRAME_CONTEXT, operations).len())
        .expect("encoded test frame length fits u64")
}

fn segment_bytes(epoch: u64, operations: &[PackOperation]) -> u64 {
    PACK_SEGMENT_HEADER_LEN + frame_bytes(epoch, operations)
}

fn segmented_config(target_bytes: u64, max_bytes: u64) -> PackStoreConfig {
    store_config(TEST_INDEX_MEMORY_BYTES)
        .with_segment_limits(target_bytes, max_bytes)
        .expect("valid focused segment limits")
}

fn commit_horizon(receipt: PackFrameReceipt) -> PackCommitHorizon {
    PackCommitHorizon {
        epoch: receipt.epoch,
        segment_id: receipt.segment_id,
        frame_end: receipt.frame_end,
        context: receipt.context,
        frame_sha256: receipt.frame_sha256,
    }
}

fn segment_path(root: &Path, id: PackSegmentId) -> std::path::PathBuf {
    root.join(id.file_name())
}

fn committed_entry(store: &PackStore, key: &[u8; PACK_KEY_BYTES]) -> IndexEntry {
    let hash = key_hash(key);
    store
        .runs
        .iter()
        .rev()
        .find_map(|live| {
            live.run
                .probe_membership(key, hash, None)
                .expect("probe committed run")
        })
        .expect("committed key has an index entry")
}

fn create_two_segment_store(root: &Path) -> (PackStoreConfig, PackFrameReceipt, PackFrameReceipt) {
    let first = [put(key(1), b"aaaa")];
    let second = [put(key(2), b"bbbb")];
    let target = segment_bytes(0, &first);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root, config).expect("create two-segment fixture");
    store
        .append_frame(TEST_FRAME_CONTEXT, &first)
        .expect("append first fixture frame");
    let first_receipt = store.last_frame_receipt().expect("first receipt");
    store
        .append_frame(TEST_FRAME_CONTEXT, &second)
        .expect("append second fixture frame");
    let second_receipt = store.last_frame_receipt().expect("second receipt");
    assert_eq!(first_receipt.segment_id, PackSegmentId::INITIAL);
    assert_eq!(second_receipt.segment_id, PackSegmentId::new(1));
    drop(store);
    (config, first_receipt, second_receipt)
}

fn open_at_horizon_error(
    root: &Path,
    config: PackStoreConfig,
    horizon: PackCommitHorizon,
) -> PackStoreError {
    match PackStore::open_at_commit_horizon(root, config, Some(horizon)) {
        Ok(_) => panic!("damaged committed segment must not open"),
        Err(error) => error,
    }
}

#[test]
fn frame_ending_exactly_at_target_stays_in_current_segment() {
    let root = tempdir().expect("temporary segmented store");
    let operations = [put(key(1), b"exact")];
    let target = segment_bytes(0, &operations);
    let mut store = PackStore::create(
        root.path(),
        segmented_config(target, target.saturating_mul(2)),
    )
    .expect("create exact-target store");

    store
        .append_frame(TEST_FRAME_CONTEXT, &operations)
        .expect("append exact-target frame");
    let receipt = store.last_frame_receipt().expect("exact-target receipt");

    assert_eq!(receipt.segment_id, PackSegmentId::INITIAL);
    assert_eq!(receipt.frame_start, PACK_SEGMENT_HEADER_LEN);
    assert_eq!(receipt.frame_end, target);
    assert_eq!(
        fs::metadata(segment_path(root.path(), receipt.segment_id))
            .unwrap()
            .len(),
        target
    );
    assert!(!segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn crossing_target_rotates_without_splitting_the_frame() {
    let root = tempdir().expect("temporary segmented store");
    let first = [put(key(1), b"aaaa")];
    let second = [put(key(2), b"bbbb")];
    let target = segment_bytes(0, &first);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root.path(), config).expect("create rotation store");

    store.append_frame(TEST_FRAME_CONTEXT, &first).unwrap();
    store.append_frame(TEST_FRAME_CONTEXT, &second).unwrap();
    let second_receipt = store.last_frame_receipt().expect("rotated receipt");

    assert_eq!(second_receipt.segment_id, PackSegmentId::new(1));
    assert_eq!(second_receipt.frame_start, PACK_SEGMENT_HEADER_LEN);
    let first_file = fs::read(segment_path(root.path(), PackSegmentId::INITIAL)).unwrap();
    let second_file = fs::read(segment_path(root.path(), PackSegmentId::new(1))).unwrap();
    assert_eq!(first_file.len() as u64, target);
    assert_eq!(second_file.len() as u64, segment_bytes(1, &second));
    assert_eq!(
        &first_file[PACK_SEGMENT_HEADER_LEN as usize..],
        encoded_test_frame(0, TEST_FRAME_CONTEXT, &first)
    );
    assert_eq!(
        &second_file[PACK_SEGMENT_HEADER_LEN as usize..],
        encoded_test_frame(1, TEST_FRAME_CONTEXT, &second)
    );
}

#[test]
fn physical_layout_counts_prepared_segments_and_recovery_removes_the_orphan() {
    let root = tempdir().expect("temporary segmented store");
    let first = [put(key(1), b"aaaa")];
    let second = [put(key(2), b"bbbb")];
    let target = segment_bytes(0, &first);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root.path(), config).expect("create layout store");
    store.append_frame(TEST_FRAME_CONTEXT, &first).unwrap();
    let committed = store.last_frame_receipt().expect("committed receipt");

    let prepared = store
        .prepare_frame(TEST_FRAME_CONTEXT, &second)
        .expect("prepare rotated frame");
    assert_eq!(prepared.receipt().segment_id, PackSegmentId::new(1));
    assert_eq!(
        store.layout().expect("prepared physical layout").0,
        target + prepared.receipt().frame_end
    );
    drop(store);

    let reopened =
        PackStore::open_at_commit_horizon(root.path(), config, Some(commit_horizon(committed)))
            .expect("discard prepared segment");
    assert_eq!(
        reopened.layout().expect("recovered physical layout").0,
        target
    );
    assert!(!segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn above_target_frame_within_the_hard_limit_gets_a_dedicated_segment() {
    let root = tempdir().expect("temporary segmented store");
    let small = [put(key(1), b"x")];
    let large = [put(key(2), &[0x5a; 512])];
    let trailing = [put(key(3), b"y")];
    let target = segment_bytes(0, &small);
    let maximum = segment_bytes(1, &large);
    assert!(maximum > target);
    let mut store = PackStore::create(root.path(), segmented_config(target, maximum))
        .expect("create dedicated-segment store");

    store.append_frame(TEST_FRAME_CONTEXT, &small).unwrap();
    store.append_frame(TEST_FRAME_CONTEXT, &large).unwrap();
    let large_receipt = store.last_frame_receipt().expect("large receipt");
    store.append_frame(TEST_FRAME_CONTEXT, &trailing).unwrap();
    let trailing_receipt = store.last_frame_receipt().expect("trailing receipt");

    assert_eq!(large_receipt.segment_id, PackSegmentId::new(1));
    assert_eq!(large_receipt.frame_start, PACK_SEGMENT_HEADER_LEN);
    assert_eq!(large_receipt.frame_end, maximum);
    assert_eq!(trailing_receipt.segment_id, PackSegmentId::new(2));
    assert_eq!(
        fs::metadata(segment_path(root.path(), PackSegmentId::new(1)))
            .unwrap()
            .len(),
        maximum
    );
}

#[test]
fn frame_above_hard_segment_limit_fails_before_creating_or_writing_a_segment() {
    let root = tempdir().expect("temporary segmented store");
    let small = [put(key(1), b"x")];
    let oversized = [put(key(2), &[0x7c; 512])];
    let target = segment_bytes(0, &small);
    let requested = segment_bytes(1, &oversized);
    let maximum = requested - 1;
    let mut store = PackStore::create(root.path(), segmented_config(target, maximum))
        .expect("create bounded-segment store");
    store.append_frame(TEST_FRAME_CONTEXT, &small).unwrap();
    let before = snapshot_store_files(root.path());

    let error = store
        .append_frame(TEST_FRAME_CONTEXT, &oversized)
        .expect_err("oversized segment frame must fail");

    assert!(matches!(
        error.downcast_ref::<PackStoreError>(),
        Some(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::SegmentBytes,
            actual,
            maximum: reported_maximum,
        }) if *actual == requested && *reported_maximum == maximum
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
    assert!(!segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn point_and_parallel_sorted_reads_route_equal_offsets_by_segment() {
    let root = tempdir().expect("temporary segmented store");
    let first: Vec<_> = (0..300)
        .map(|number| put(numbered_key(number), &[0x11; 8]))
        .collect();
    let second: Vec<_> = (10_000..10_300)
        .map(|number| put(numbered_key(number), &[0x22; 8]))
        .collect();
    let target = segment_bytes(0, &first);
    assert_eq!(frame_bytes(0, &first), frame_bytes(1, &second));
    let options = PackStoreOptions {
        random_point_mmap: true,
        batch_value_workers: 2,
    };
    let config = segmented_config(target, target.saturating_mul(2))
        .with_read_options(options)
        .expect("valid parallel read options");
    let mut store = PackStore::create(root.path(), config).expect("create batch-read store");
    store.append_frame(TEST_FRAME_CONTEXT, &first).unwrap();
    store.append_frame(TEST_FRAME_CONTEXT, &second).unwrap();

    let first_key = numbered_key(0);
    let second_key = numbered_key(10_000);
    let first_entry = committed_entry(&store, &first_key);
    let second_entry = committed_entry(&store, &second_key);
    assert_eq!(first_entry.segment_id, PackSegmentId::INITIAL);
    assert_eq!(second_entry.segment_id, PackSegmentId::new(1));
    assert_eq!(first_entry.value_offset, second_entry.value_offset);
    assert_eq!(store.get(&first_key).unwrap(), Some(vec![0x11; 8]));
    assert_eq!(store.get(&second_key).unwrap(), Some(vec![0x22; 8]));

    let keys: Vec<_> = (0..300)
        .map(numbered_key)
        .chain((10_000..10_300).map(numbered_key))
        .collect();
    assert!(keys.len() >= options.batch_value_parallel_threshold());
    let values = store
        .get_many_sorted(&keys)
        .expect("cross-segment sorted read");
    assert_eq!(values[..300], vec![Some(vec![0x11; 8]); 300]);
    assert_eq!(values[300..], vec![Some(vec![0x22; 8]); 300]);
}

#[test]
fn snapshot_pinned_before_rotation_remains_stable() {
    let root = tempdir().expect("temporary segmented store");
    let target_key = key(1);
    let added_key = key(2);
    let first = [put(target_key, b"old")];
    let second = [put(target_key, b"new"), put(added_key, b"add")];
    let target = segment_bytes(0, &first);
    let maximum = segment_bytes(1, &second);
    let mut store = PackStore::create(root.path(), segmented_config(target, maximum)).unwrap();
    store.append_frame(TEST_FRAME_CONTEXT, &first).unwrap();
    let pinned = store.snapshot().expect("pin pre-rotation snapshot");

    store.append_frame(TEST_FRAME_CONTEXT, &second).unwrap();
    assert_eq!(
        store.last_frame_receipt().unwrap().segment_id,
        PackSegmentId::new(1)
    );
    assert_eq!(pinned.get(&target_key).unwrap(), Some(b"old".to_vec()));
    assert_eq!(pinned.get(&added_key).unwrap(), None);
    assert_eq!(store.get(&target_key).unwrap(), Some(b"new".to_vec()));
    assert_eq!(store.get(&added_key).unwrap(), Some(b"add".to_vec()));
}

#[test]
fn compaction_preserves_values_positioned_in_old_segments() {
    let root = tempdir().expect("temporary segmented store");
    let frames = [
        [put(key(1), b"aaaa")],
        [put(key(2), b"bbbb")],
        [put(key(3), b"cccc")],
    ];
    let target = segment_bytes(0, &frames[0]);
    let config = segmented_config(target, target.saturating_mul(2))
        .with_compaction_bounds(2, 2, 3)
        .expect("valid focused compaction bounds");
    let mut store = PackStore::create(root.path(), config).unwrap();
    for frame in &frames {
        append_without_maintenance(&mut store, frame);
    }

    let plan = store
        .plan_compaction()
        .unwrap()
        .expect("L0 compaction plan");
    let prepared = plan.build().expect("build cross-segment compaction");
    store
        .adopt_compaction(prepared)
        .expect("adopt cross-segment compaction");

    assert_eq!(store.get(&key(1)).unwrap(), Some(b"aaaa".to_vec()));
    assert_eq!(store.get(&key(2)).unwrap(), Some(b"bbbb".to_vec()));
    assert_eq!(
        committed_entry(&store, &key(1)).segment_id,
        PackSegmentId::INITIAL
    );
    assert_eq!(
        committed_entry(&store, &key(2)).segment_id,
        PackSegmentId::new(1)
    );
}

#[test]
fn later_segment_reopens_standalone_and_at_commit_horizon() {
    let root = tempdir().expect("temporary segmented store");
    let (config, _, latest) = create_two_segment_store(root.path());

    let reopened = PackStore::open(root.path(), config).expect("standalone reopen");
    assert_eq!(reopened.last_frame_receipt(), Some(latest));
    assert_eq!(reopened.get(&key(1)).unwrap(), Some(b"aaaa".to_vec()));
    assert_eq!(reopened.get(&key(2)).unwrap(), Some(b"bbbb".to_vec()));
    drop(reopened);

    let reopened =
        PackStore::open_at_commit_horizon(root.path(), config, Some(commit_horizon(latest)))
            .expect("horizon reopen through later segment");
    assert_eq!(reopened.last_frame_receipt(), Some(latest));
    assert_eq!(reopened.get(&key(2)).unwrap(), Some(b"bbbb".to_vec()));
}

#[test]
fn earlier_commit_horizon_removes_an_orphan_later_segment() {
    let root = tempdir().expect("temporary segmented store");
    let first = [put(key(1), b"aaaa")];
    let orphan = [put(key(2), b"bbbb")];
    let target = segment_bytes(0, &first);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root.path(), config).unwrap();
    store.append_frame(TEST_FRAME_CONTEXT, &first).unwrap();
    let committed = store.last_frame_receipt().expect("committed receipt");
    let prepared = store.prepare_frame(TEST_FRAME_CONTEXT, &orphan).unwrap();
    let sealed = store.seal_prepared(prepared).expect("seal orphan fixture");
    assert_eq!(sealed.commit_horizon().segment_id, PackSegmentId::new(1));
    assert!(segment_path(root.path(), PackSegmentId::new(1)).exists());
    drop(sealed);
    drop(store);

    let reopened =
        PackStore::open_at_commit_horizon(root.path(), config, Some(commit_horizon(committed)))
            .expect("discard orphan later segment");
    assert_eq!(reopened.last_frame_receipt(), Some(committed));
    assert_eq!(reopened.get(&key(1)).unwrap(), Some(b"aaaa".to_vec()));
    assert_eq!(reopened.get(&key(2)).unwrap(), None);
    assert!(!segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn missing_or_corrupt_committed_later_segment_fails_without_mutation() {
    let missing_root = tempdir().expect("temporary missing-segment store");
    let (missing_config, _, missing_tip) = create_two_segment_store(missing_root.path());
    fs::remove_file(segment_path(missing_root.path(), PackSegmentId::new(1)))
        .expect("remove committed later segment");
    let missing_before = snapshot_store_files(missing_root.path());
    let missing_error = open_at_horizon_error(
        missing_root.path(),
        missing_config,
        commit_horizon(missing_tip),
    );
    assert!(missing_error.to_string().contains("segment"));
    assert_eq!(snapshot_store_files(missing_root.path()), missing_before);

    let corrupt_root = tempdir().expect("temporary corrupt-segment store");
    let (corrupt_config, _, corrupt_tip) = create_two_segment_store(corrupt_root.path());
    let path = segment_path(corrupt_root.path(), PackSegmentId::new(1));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let offset = corrupt_tip.frame_start + FRAME_HEADER_LEN as u64;
    let mut byte = [0u8; 1];
    file.read_exact_at(&mut byte, offset).unwrap();
    file.write_all_at(&[byte[0] ^ 0x80], offset).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let corrupt_before = snapshot_store_files(corrupt_root.path());
    let corrupt_error = open_at_horizon_error(
        corrupt_root.path(),
        corrupt_config,
        commit_horizon(corrupt_tip),
    );
    assert!(
        corrupt_error.to_string().contains("corrupt")
            || corrupt_error.to_string().contains("checksum")
    );
    assert_eq!(snapshot_store_files(corrupt_root.path()), corrupt_before);
}

#[test]
fn reopen_enforces_configured_frame_bounds_before_mutation() {
    let root = tempdir().expect("temporary bounded-reopen store");
    let config = store_config(TEST_INDEX_MEMORY_BYTES);
    let mut store = PackStore::create(root.path(), config).expect("create bounded-reopen store");
    store
        .append_frame(
            TEST_FRAME_CONTEXT,
            &[put(key(1), b"one"), put(key(2), b"two")],
        )
        .expect("append two-row frame");
    drop(store);
    let before = snapshot_store_files(root.path());
    let constrained = config
        .with_max_frame_rows(1)
        .expect("valid one-row resource contract");

    let error = PackStore::open(root.path(), constrained)
        .err()
        .expect("persisted frame above the configured row bound must fail");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::FrameRows,
            actual: 2,
            maximum: 1,
        }
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
}

#[test]
fn oversized_orphan_segment_fails_before_horizon_cleanup() {
    let root = tempdir().expect("temporary oversized-orphan store");
    let committed_ops = [put(key(1), b"x")];
    let orphan_ops = [put(key(2), &[0x5a; 512])];
    let committed_bytes = segment_bytes(0, &committed_ops);
    let orphan_bytes = segment_bytes(1, &orphan_ops);
    assert!(orphan_bytes > committed_bytes);
    let writer_config = segmented_config(committed_bytes, orphan_bytes);
    let mut store = PackStore::create(root.path(), writer_config).expect("create orphan fixture");
    store
        .append_frame(TEST_FRAME_CONTEXT, &committed_ops)
        .expect("append committed frame");
    let committed = store.last_frame_receipt().expect("committed receipt");
    let prepared = store
        .prepare_frame(TEST_FRAME_CONTEXT, &orphan_ops)
        .expect("prepare oversized orphan");
    let sealed = store
        .seal_prepared(prepared)
        .expect("seal oversized orphan");
    drop(sealed);
    drop(store);
    let before = snapshot_store_files(root.path());
    let reader_config = segmented_config(committed_bytes, committed_bytes);

    let error = PackStore::open_at_commit_horizon(
        root.path(),
        reader_config,
        Some(commit_horizon(committed)),
    )
    .err()
    .expect("oversized orphan segment must fail before cleanup");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::SegmentBytes,
            actual,
            maximum,
        } if actual == orphan_bytes && maximum == committed_bytes
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
    assert!(segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn unknown_frame_version_in_later_orphan_segment_fails_without_mutation() {
    let root = tempdir().expect("temporary unknown-version orphan store");
    let committed_ops = [put(key(1), b"committed")];
    let orphan_ops = [put(key(2), b"orphan")];
    let target = segment_bytes(0, &committed_ops);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root.path(), config).expect("create orphan fixture");
    store
        .append_frame(TEST_FRAME_CONTEXT, &committed_ops)
        .expect("append committed frame");
    let committed = store.last_frame_receipt().expect("committed receipt");
    let prepared = store
        .prepare_frame(TEST_FRAME_CONTEXT, &orphan_ops)
        .expect("prepare orphan frame");
    let orphan_receipt = prepared.receipt();
    let sealed = store.seal_prepared(prepared).expect("seal orphan frame");
    let orphan = sealed.commit_horizon();
    assert_eq!(orphan.segment_id, PackSegmentId::new(1));
    drop(sealed);
    drop(store);

    let path = segment_path(root.path(), orphan.segment_id);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open orphan segment");
    file.write_all_at(b"N3PACK99", orphan_receipt.frame_start)
        .expect("write unknown orphan frame magic");
    file.sync_all().expect("sync unknown orphan frame magic");
    drop(file);
    let before = snapshot_store_files(root.path());

    let error =
        PackStore::open_at_commit_horizon(root.path(), config, Some(commit_horizon(committed)))
            .err()
            .expect("unknown orphan frame version must fail closed");

    assert!(matches!(
        error,
        PackStoreError::UnsupportedVersion {
            artifact: PackStoreArtifact::Frame,
            found: 99,
            ..
        }
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
}

#[test]
fn corrupt_middle_run_record_is_detected_before_orphan_cleanup() {
    let root = tempdir().expect("temporary middle-record recovery store");
    let committed_ops = [
        put(key(1), b"one"),
        put(key(2), b"two"),
        put(key(3), b"three"),
    ];
    let orphan_ops = [put(key(4), b"orphan")];
    let target = segment_bytes(0, &committed_ops);
    let config = segmented_config(target, target.saturating_mul(2));
    let mut store = PackStore::create(root.path(), config).expect("create recovery fixture");
    store
        .append_frame(TEST_FRAME_CONTEXT, &committed_ops)
        .expect("append committed frame");
    let run_path = root.path().join("runs").join(run_file_name(0, 0, 0));
    let records_offset = store.runs[0].run.records_offset;
    let resident_bytes = store.runs[0].run.memory_bytes;
    let rows = committed_ops.len() as u64;
    let metadata_bytes = rows * FRAME_ROW_METADATA_LEN as u64;
    let rebuild_peak =
        recovery::estimate_rebuild_peak_bytes(0, metadata_bytes, rows, committed_ops.len())
            .expect("estimate rebuild peak");
    let rejected_bound = rebuild_peak.checked_sub(1).expect("positive rebuild peak");
    assert!(rejected_bound >= resident_bytes);
    let prepared = store
        .prepare_frame(TEST_FRAME_CONTEXT, &orphan_ops)
        .expect("prepare later orphan");
    let sealed = store.seal_prepared(prepared).expect("seal later orphan");
    assert_eq!(sealed.commit_horizon().segment_id, PackSegmentId::new(1));
    drop(sealed);
    drop(store);

    flip_persisted_run_byte(&run_path, records_offset + INDEX_RECORD_LEN as u64 + 1);
    let before = snapshot_store_files(root.path());
    let constrained = config
        .with_max_index_memory_bytes(rejected_bound)
        .expect("valid constrained recovery memory");

    let error = PackStore::open(root.path(), constrained)
        .err()
        .expect("corrupt middle record must force a bounded rebuild");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::IndexMemoryBytes,
            actual,
            maximum,
        } if actual == rebuild_peak && maximum == rejected_bound
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
    assert!(segment_path(root.path(), PackSegmentId::new(1)).exists());
}

#[test]
fn manifest_run_count_limit_fails_without_mutation() {
    let root = tempdir().expect("temporary run-count recovery store");
    let config = store_config(TEST_INDEX_MEMORY_BYTES);
    let mut store = PackStore::create(root.path(), config).expect("create run-count fixture");
    append_without_maintenance(&mut store, &[put(key(1), b"one")]);
    append_without_maintenance(&mut store, &[put(key(2), b"two")]);
    assert_eq!(store.runs.len(), 2);
    drop(store);
    let before = snapshot_store_files(root.path());
    let constrained = config
        .with_max_recent_runs(1)
        .expect("valid one-run recovery limit");

    let error = PackStore::open(root.path(), constrained)
        .err()
        .expect("manifest above configured run count must fail");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::RecentRuns,
            actual: 2,
            maximum: 1,
        }
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
}

#[test]
fn manifest_index_level_limit_fails_without_mutation() {
    let root = tempdir().expect("temporary level-count recovery store");
    let config = small_compaction_config(TEST_INDEX_MEMORY_BYTES);
    let mut store = PackStore::create(root.path(), config).expect("create level-count fixture");
    for tag in 0..9u8 {
        store
            .append_frame(TEST_FRAME_CONTEXT, &[put(key(tag), &[tag])])
            .expect("append compacted fixture frame");
    }
    let level_count = store
        .runs
        .iter()
        .map(|live| live.level)
        .max()
        .expect("fixture has runs")
        + 1;
    assert!(level_count > 2);
    drop(store);
    let before = snapshot_store_files(root.path());
    let constrained = config
        .with_max_index_levels(2)
        .expect("valid two-level recovery limit");

    let error = PackStore::open(root.path(), constrained)
        .err()
        .expect("manifest above configured level count must fail");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::IndexLevels,
            actual,
            maximum: 2,
        } if actual == u64::from(level_count)
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
}

#[test]
fn manifest_decoded_index_limit_fails_without_mapping_or_mutation() {
    let root = tempdir().expect("temporary decoded-index recovery store");
    let config = store_config(TEST_INDEX_MEMORY_BYTES);
    let operations: Vec<_> = (0..130u32)
        .map(|number| put(numbered_key(number), b"value"))
        .collect();
    let mut store = PackStore::create(root.path(), config).expect("create index-memory fixture");
    store
        .append_frame(TEST_FRAME_CONTEXT, &operations)
        .expect("append indexed fixture frame");
    let decoded_bytes = store.runs[0].run.memory_bytes;
    drop(store);
    let before = snapshot_store_files(root.path());
    let maximum = decoded_bytes
        .checked_sub(1)
        .expect("positive decoded run size");
    let constrained = config
        .with_max_index_memory_bytes(maximum)
        .expect("valid constrained decoded-index limit");

    let error = PackStore::open(root.path(), constrained)
        .err()
        .expect("manifest above decoded-index limit must fail");

    assert!(matches!(
        error,
        PackStoreError::LimitExceeded {
            limit: PackStoreLimit::IndexMemoryBytes,
            actual,
            maximum: configured,
        } if actual == decoded_bytes && configured == maximum
    ));
    assert_eq!(snapshot_store_files(root.path()), before);
}
