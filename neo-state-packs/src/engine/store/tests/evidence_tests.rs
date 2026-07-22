#[test]
fn materialized_evidence_is_stable_across_compaction_and_reopen() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), small_compaction_config(1024 * 1024)).expect("create store");
    let updated = key(90);
    let deleted = key(91);
    append_without_maintenance(&mut store, &[put(updated, b"v1"), put(key(92), b"stable")]);
    append_without_maintenance(&mut store, &[put(updated, b"v2"), put(deleted, b"gone")]);
    append_without_maintenance(&mut store, &[tombstone(deleted), put(key(93), b"new")]);

    let before = store
        .materialized_view_evidence(64)
        .expect("pre-compaction evidence");
    let repeated = store
        .materialized_view_evidence(64)
        .expect("repeat deterministic evidence");
    assert!(before.state_matches(&repeated));

    let mut other_segment = repeated;
    other_segment.tip_segment_id = PackSegmentId::new(
        repeated
            .tip_segment_id
            .get()
            .checked_add(1)
            .expect("fixture segment identity increments"),
    );
    assert!(!before.state_matches(&other_segment));
    assert_eq!(before.generation, repeated.generation);
    assert_eq!(before.live_runs, repeated.live_runs);
    assert_eq!(before.source_records, repeated.source_records);
    assert_eq!(before.live_runs, 3);
    assert_eq!(before.source_records, 6);
    assert_eq!(before.winner_records, 4);
    assert_eq!(before.puts, 3);
    assert_eq!(before.tombstones, 1);
    assert_eq!(before.lookup_sampled_keys, 4);
    assert_eq!(before.lookup_present, 3);
    assert_eq!(before.lookup_absent, 1);
    assert_eq!(before.point_checks, 4);
    assert_eq!(before.synthetic_miss_checks, 256);
    assert_eq!(before.lookup_batches, 2);
    assert_eq!(before.frame_reference_keys, 4);
    assert_eq!(before.frame_scrub.frames, 3);
    assert_eq!(before.frame_scrub.rows, 6);

    let plan = store
        .plan_compaction()
        .expect("plan compaction")
        .expect("overfull L0 has a plan");
    let prepared = plan.build().expect("build compaction");
    let preview = store
        .prepared_compaction_evidence(&prepared, 64)
        .expect("pre-adoption compaction evidence");
    assert!(before.state_matches(&preview));
    assert_eq!(preview.live_runs, 1);
    assert_eq!(preview.generation, before.generation + 1);
    store
        .scrub_prepared_compaction(&prepared)
        .expect("scrub pre-adoption output");
    let still_current = store
        .materialized_view_evidence(64)
        .expect("current evidence remains unchanged before adoption");
    assert_eq!(still_current.generation, before.generation);
    assert_eq!(still_current.live_runs, before.live_runs);
    store.adopt_compaction(prepared).expect("adopt compaction");
    let after = store
        .materialized_view_evidence(64)
        .expect("post-compaction evidence");
    assert!(before.state_matches(&after));
    assert_eq!(after.live_runs, 1);
    assert_eq!(after.source_records, 4);
    assert_ne!(before.generation, after.generation);

    drop(store);
    let reopened =
        PackStore::open(root.path(), store_config(1024 * 1024)).expect("reopen compacted store");
    let reopened_evidence = reopened
        .materialized_view_evidence(64)
        .expect("reopened evidence");
    assert!(after.state_matches(&reopened_evidence));
    assert_eq!(
        after.winner_records_sha256,
        reopened_evidence.winner_records_sha256
    );
    assert_eq!(
        after.frame_reference_sha256,
        reopened_evidence.frame_reference_sha256
    );
    assert_eq!(after.lookup_sha256, reopened_evidence.lookup_sha256);
}

#[test]
fn checkpoint_index_evidence_binds_complete_frame_and_winner_records() {
    let root = tempdir().expect("temporary checkpoint pack");
    let mut store =
        PackStore::create(root.path(), small_compaction_config(1024 * 1024)).expect("create store");
    append_without_maintenance(
        &mut store,
        &[put(numbered_key(1), b"one"), put(numbered_key(2), b"two")],
    );
    append_without_maintenance(
        &mut store,
        &[
            put(numbered_key(3), b"three"),
            put(numbered_key(4), b"four"),
        ],
    );
    append_without_maintenance(&mut store, &[put(numbered_key(5), b"five")]);

    let before = store
        .checkpoint_index_evidence()
        .expect("put-only checkpoint indexes must bind to frame rows");
    assert_eq!(before.frame_records, 5);
    assert_eq!(before.winner_records, 5);
    assert_eq!(before.value_bytes, 19);
    assert_eq!(before.live_runs, 3);
    assert_eq!(before.source_records, 5);

    let plan = store
        .plan_compaction()
        .expect("plan compaction")
        .expect("overfull L0 has a plan");
    let prepared = plan.build().expect("build compaction");
    store.adopt_compaction(prepared).expect("adopt compaction");
    let compacted = store
        .checkpoint_index_evidence()
        .expect("compacted checkpoint indexes remain bound");
    assert_eq!(compacted.records_sha256, before.records_sha256);
    assert_eq!(compacted.live_runs, 1);
    assert_eq!(compacted.source_records, 5);

    drop(store);
    let reopened =
        PackStore::open(root.path(), small_compaction_config(1024 * 1024)).expect("reopen pack");
    assert_eq!(
        reopened
            .checkpoint_index_evidence()
            .expect("reopened checkpoint indexes remain bound"),
        compacted
    );
}

#[test]
fn checkpoint_index_evidence_rejects_non_checkpoint_version_streams() {
    let root = tempdir().expect("temporary runtime pack");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    append_without_maintenance(&mut store, &[put(numbered_key(1), b"present")]);
    append_without_maintenance(&mut store, &[tombstone(numbered_key(1))]);

    let error = store
        .checkpoint_index_evidence()
        .expect_err("checkpoint evidence must reject tombstones and repeated keys");
    assert!(
        error.to_string().contains("put-only")
            || error.to_string().contains("globally unique ordered")
    );
}

#[test]
fn materialized_evidence_compares_lookup_results_with_winner_offsets() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store.append_frame(TEST_FRAME_CONTEXT, &[put(key(95), b"value")]).expect("append");
    store
        .materialized_view_evidence(1)
        .expect("baseline evidence");

    store.ranges[0].min_prefix = u64::MAX;
    let error = store
        .materialized_view_evidence(1)
        .expect_err("range-routing corruption must not match winner evidence");
    assert!(error.to_string().contains("winner record"));
}

#[test]
fn materialized_evidence_rejects_an_unbounded_sample_before_work() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store.append_frame(TEST_FRAME_CONTEXT, &[put(key(94), b"value")]).expect("append");
    let error = store
        .materialized_view_evidence(1_000_001)
        .expect_err("unbounded sample must fail");
    assert!(error.to_string().contains("hard limit"));
}

#[test]
fn materialized_evidence_bounds_large_value_lookup_batches() {
    const LARGE_VALUE_BYTES: usize = 65_539;
    const LARGE_VALUES: u32 = 260;

    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let value = vec![0xA5; LARGE_VALUE_BYTES];
    let operations: Vec<_> = (0..LARGE_VALUES)
        .map(|ordinal| {
            let mut operation_key = [0u8; PACK_KEY_BYTES];
            operation_key[0] = TEST_NODE_PREFIX;
            operation_key[PACK_KEY_BYTES - 4..].copy_from_slice(&ordinal.to_be_bytes());
            put(operation_key, &value)
        })
        .collect();
    store.append_frame(TEST_FRAME_CONTEXT, &operations).expect("append large values");

    let evidence = store
        .materialized_view_evidence(LARGE_VALUES as usize)
        .expect("bounded large-value evidence");
    assert_eq!(evidence.lookup_sampled_keys, u64::from(LARGE_VALUES));
    assert_eq!(evidence.lookup_present, u64::from(LARGE_VALUES));
    assert_eq!(evidence.lookup_absent, 0);
    assert_eq!(
        evidence.lookup_value_bytes,
        u64::from(LARGE_VALUES) * LARGE_VALUE_BYTES as u64
    );
    assert_eq!(evidence.synthetic_miss_checks, 256);
    assert!(
        evidence.lookup_batches >= 3,
        "large values require at least two value batches plus the miss batch"
    );
}

#[test]
fn materialized_evidence_hashes_a_multichunk_frame_reference_value() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(8 * 1024 * 1024)).expect("create store");
    let value = vec![0x5A; evidence::FRAME_REFERENCE_VALUE_HASH_CHUNK_BYTES + 17];
    store
        .append_frame(TEST_FRAME_CONTEXT, &[put(key(97), &value)])
        .expect("append multi-chunk value");

    let evidence = store
        .materialized_view_evidence(1)
        .expect("multi-chunk evidence");
    assert_eq!(evidence.frame_reference_keys, 1);
    assert_eq!(evidence.lookup_present, 1);
    assert_eq!(evidence.lookup_value_bytes, value.len() as u64);
}

#[test]
fn lookup_batch_rejects_one_value_above_the_byte_limit() {
    let entries = [IndexEntry {
        key: key(96),
        sequence: 0,
        segment_id: PackSegmentId::INITIAL,
        value_offset: 0,
        value_len: 16 * 1024 * 1024 + 1,
        tombstone: false,
    }];
    let error = evidence::next_lookup_batch(&entries, 0)
        .expect_err("oversized sampled value must fail closed");
    assert!(
        error
            .to_string()
            .contains("exceeds the sorted lookup batch limit")
    );
}
