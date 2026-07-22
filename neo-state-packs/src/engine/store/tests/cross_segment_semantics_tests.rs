use super::*;
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;

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

fn compacting_segmented_config(target_bytes: u64, max_bytes: u64) -> PackStoreConfig {
    small_compaction_config(TEST_INDEX_MEMORY_BYTES)
        .with_segment_limits(target_bytes, max_bytes)
        .expect("valid compacting segment limits")
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

#[test]
fn sequential_sorted_reads_distinguish_equal_offsets_in_different_segments() {
    let root = tempdir().expect("temporary sequential-read store");
    let first_key = key(1);
    let second_key = key(2);
    let first = [put(first_key, b"first")];
    let second = [put(second_key, b"other")];
    let target = segment_bytes(0, &first);
    assert_eq!(frame_bytes(0, &first), frame_bytes(1, &second));
    let config = segmented_config(target, target);
    assert_eq!(config.read_options().batch_value_workers, 1);
    let mut store = PackStore::create(root.path(), config).expect("create sequential-read store");

    append_without_maintenance(&mut store, &first);
    append_without_maintenance(&mut store, &second);

    let first_entry = committed_entry(&store, &first_key);
    let second_entry = committed_entry(&store, &second_key);
    assert_eq!(first_entry.segment_id, PackSegmentId::INITIAL);
    assert_eq!(second_entry.segment_id, PackSegmentId::new(1));
    assert_eq!(first_entry.value_offset, second_entry.value_offset);
    assert_eq!(first_entry.value_len, second_entry.value_len);
    assert_eq!(store.get(&first_key).unwrap(), Some(b"first".to_vec()));
    assert_eq!(store.get(&second_key).unwrap(), Some(b"other".to_vec()));
    assert_eq!(
        store
            .get_many_sorted(&[first_key, second_key])
            .expect("sequential cross-segment batch read"),
        vec![Some(b"first".to_vec()), Some(b"other".to_vec())]
    );
}

#[test]
fn empty_put_and_tombstone_survive_cross_segment_compaction_and_reopen() {
    let root = tempdir().expect("temporary empty/tombstone store");
    let empty_key = key(20);
    let deleted_key = key(21);
    let stable_key = key(22);
    let tail_key = key(23);
    let first = [put(empty_key, b""), put(deleted_key, b"present")];
    let second = [tombstone(deleted_key), put(stable_key, b"stable")];
    let third = [put(tail_key, b"tail")];
    let target = segment_bytes(0, &first);
    let maximum = [target, segment_bytes(1, &second), segment_bytes(2, &third)]
        .into_iter()
        .max()
        .expect("non-empty segment sizes");
    let config = compacting_segmented_config(target, maximum);
    let mut store = PackStore::create(root.path(), config).expect("create semantic store");

    append_without_maintenance(&mut store, &first);
    append_without_maintenance(&mut store, &second);
    append_without_maintenance(&mut store, &third);
    assert_eq!(
        store
            .last_frame_receipt()
            .expect("third receipt")
            .segment_id,
        PackSegmentId::new(2)
    );

    let empty_entry = committed_entry(&store, &empty_key);
    assert!(!empty_entry.tombstone);
    assert_eq!(empty_entry.segment_id, PackSegmentId::INITIAL);
    assert!(empty_entry.value_offset >= PACK_SEGMENT_HEADER_LEN);
    assert_eq!(empty_entry.value_len, 0);
    let deleted_entry = committed_entry(&store, &deleted_key);
    assert!(deleted_entry.tombstone);
    assert_eq!(deleted_entry.segment_id, PackSegmentId::INITIAL);
    assert_eq!(deleted_entry.value_offset, 0);
    assert_eq!(deleted_entry.value_len, 0);

    let keys = [empty_key, deleted_key, stable_key, tail_key];
    let expected = vec![
        Some(Vec::new()),
        None,
        Some(b"stable".to_vec()),
        Some(b"tail".to_vec()),
    ];
    assert_eq!(store.get(&empty_key).unwrap(), Some(Vec::new()));
    assert_eq!(store.get(&deleted_key).unwrap(), None);
    assert_eq!(store.get_many_sorted(&keys).unwrap(), expected);

    let plan = store
        .plan_compaction()
        .expect("plan semantic compaction")
        .expect("three level-zero runs require compaction");
    let prepared = plan.build().expect("build semantic compaction");
    store
        .adopt_compaction(prepared)
        .expect("adopt semantic compaction");
    assert_eq!(store.get(&empty_key).unwrap(), Some(Vec::new()));
    assert_eq!(store.get(&deleted_key).unwrap(), None);
    assert_eq!(store.get_many_sorted(&keys).unwrap(), expected);

    drop(store);
    let reopened = PackStore::open(root.path(), config).expect("reopen semantic store");
    assert_eq!(reopened.get(&empty_key).unwrap(), Some(Vec::new()));
    assert_eq!(reopened.get(&deleted_key).unwrap(), None);
    assert_eq!(reopened.get_many_sorted(&keys).unwrap(), expected);
}

#[test]
fn scrub_and_materialized_evidence_cover_all_committed_segments() {
    let root = tempdir().expect("temporary multi-segment evidence store");
    let first = [put(key(30), b"one"), put(key(31), b"two")];
    let second = [put(key(30), b"updated"), tombstone(key(31))];
    let third = [put(key(32), b"three"), put(key(33), b"")];
    let target = segment_bytes(0, &first);
    let maximum = [target, segment_bytes(1, &second), segment_bytes(2, &third)]
        .into_iter()
        .max()
        .expect("non-empty segment sizes");
    let config = segmented_config(target, maximum);
    let mut store = PackStore::create(root.path(), config).expect("create evidence store");

    append_without_maintenance(&mut store, &first);
    append_without_maintenance(&mut store, &second);
    append_without_maintenance(&mut store, &third);
    assert_eq!(
        store
            .last_frame_receipt()
            .expect("third receipt")
            .segment_id,
        PackSegmentId::new(2)
    );

    let frame_scrub = store
        .scrub_committed_frames()
        .expect("scrub all committed segments");
    assert_eq!(
        frame_scrub,
        PackScrubStats {
            frames: 3,
            rows: 6,
            puts: 5,
            tombstones: 1,
            payload_bytes: frame_scrub.payload_bytes,
            value_bytes: 18,
        }
    );
    assert!(frame_scrub.payload_bytes > frame_scrub.value_bytes);
    assert_eq!(
        store.scrub_index_runs().expect("scrub all index runs"),
        PackIndexScrubStats {
            runs: 3,
            v5_runs: 3,
            records: 6,
            record_bytes: 6 * INDEX_RECORD_LEN as u64,
        }
    );

    let evidence = store
        .materialized_view_evidence(16)
        .expect("materialize all segment winners");
    assert_eq!(evidence.tip_segment_id, PackSegmentId::new(2));
    assert_eq!(evidence.source_records, 6);
    assert_eq!(evidence.winner_records, 4);
    assert_eq!(evidence.puts, 3);
    assert_eq!(evidence.tombstones, 1);
    assert_eq!(evidence.value_bytes, 12);
    assert_eq!(evidence.lookup_sampled_keys, 4);
    assert_eq!(evidence.lookup_present, 3);
    assert_eq!(evidence.lookup_absent, 1);
    assert_eq!(evidence.frame_reference_keys, 4);
    assert_eq!(evidence.frame_scrub, frame_scrub);

    drop(store);
    let reopened = PackStore::open(root.path(), config).expect("reopen evidence store");
    let reopened_scrub = reopened
        .scrub_committed_frames()
        .expect("scrub reopened segments");
    assert_eq!(reopened_scrub, frame_scrub);
    let reopened_evidence = reopened
        .materialized_view_evidence(16)
        .expect("materialize reopened segment winners");
    assert!(evidence.state_matches(&reopened_evidence));
}

#[test]
fn streaming_scrub_revalidates_sealed_headers_without_invalidating_pinned_lookup() {
    let root = tempdir().expect("temporary scrub-header store");
    let sealed_key = key(34);
    let tip_key = key(35);
    let first = [put(sealed_key, b"sealed")];
    let second = [put(tip_key, b"tipseg")];
    let target = segment_bytes(0, &first);
    assert_eq!(target, segment_bytes(1, &second));
    let config = segmented_config(target, target);
    let mut store = PackStore::create(root.path(), config).expect("create scrub-header store");

    append_without_maintenance(&mut store, &first);
    append_without_maintenance(&mut store, &second);
    assert_eq!(
        store
            .last_frame_receipt()
            .expect("second receipt")
            .segment_id,
        PackSegmentId::new(1)
    );
    drop(store);

    let store = PackStore::open(root.path(), config).expect("reopen scrub-header store");
    let pinned = store.snapshot().expect("pin valid cross-segment view");
    assert_eq!(pinned.get(&sealed_key).unwrap(), Some(b"sealed".to_vec()));
    assert_eq!(pinned.get(&tip_key).unwrap(), Some(b"tipseg".to_vec()));

    let sealed_path = root.path().join(PackSegmentId::INITIAL.file_name());
    let sealed_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&sealed_path)
        .expect("open sealed segment for corruption");
    sealed_file
        .write_all_at(b"X", 0)
        .expect("corrupt sealed segment magic");
    sealed_file
        .sync_all()
        .expect("sync sealed segment corruption");

    let scrub_error = store
        .scrub_committed_frames()
        .expect_err("streaming scrub must revalidate the sealed segment header");
    assert!(scrub_error.to_string().contains("invalid magic"));
    assert_eq!(pinned.get(&sealed_key).unwrap(), Some(b"sealed".to_vec()));
    assert_eq!(
        pinned
            .get_many_sorted(&[sealed_key, tip_key])
            .expect("pinned lookup after scrub failure"),
        vec![Some(b"sealed".to_vec()), Some(b"tipseg".to_vec())]
    );
}

#[test]
fn metrics_count_a_durable_rotated_frame_before_activation() {
    let root = tempdir().expect("temporary prepared-metrics store");
    let committed_key = key(40);
    let prepared_key = key(41);
    let first = [put(committed_key, b"aaaa")];
    let second = [put(prepared_key, b"bbbb")];
    let target = segment_bytes(0, &first);
    assert_eq!(target, segment_bytes(1, &second));
    let config = segmented_config(target, target);
    let mut store = PackStore::create(root.path(), config).expect("create metrics store");
    store
        .append_frame(TEST_FRAME_CONTEXT, &first)
        .expect("append committed frame");
    let committed = store.last_frame_receipt().expect("committed receipt");
    assert_eq!(
        store
            .metrics()
            .expect("committed metrics")
            .physical_pack_bytes,
        target
    );

    let prepared = store
        .prepare_frame(TEST_FRAME_CONTEXT, &second)
        .expect("prepare durable rotated frame");
    assert_eq!(prepared.receipt().segment_id, PackSegmentId::new(1));
    assert_eq!(prepared.receipt().frame_end, target);
    let prepared_metrics = store.metrics().expect("prepared physical metrics");
    assert_eq!(prepared_metrics.physical_pack_bytes, target * 2);
    assert_eq!(prepared_metrics.live_runs, 1);
    assert_eq!(store.last_frame_receipt(), Some(committed));
    assert_eq!(store.get(&committed_key).unwrap(), Some(b"aaaa".to_vec()));
    assert_eq!(store.get(&prepared_key).unwrap(), None);

    drop(store);
    let reopened = PackStore::open(root.path(), config).expect("discard unpublished frame");
    assert_eq!(
        reopened
            .metrics()
            .expect("recovered physical metrics")
            .physical_pack_bytes,
        target
    );
    assert_eq!(reopened.get(&prepared_key).unwrap(), None);
}
