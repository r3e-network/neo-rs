//! # neo-storage::tests::mdbx
//!
//! Test module grouping the production-default MDBX provider and store adapter.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-storage; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::store::estimate_cursor_write_us;
use super::*;
use crate::persistence::providers::RuntimeStore;
use crate::persistence::{
    CoordinatedTransactionalStore, RawOverlaySink, RawOverlaySource, RawReadOnlyStore,
    ReadOnlyStoreGeneric, SeekDirection, Store, StoreCache, StoreMaintenanceBatch, StoreSnapshot,
    TransactionalStore, WriteStore, storage::StorageConfig,
};
use crate::{StorageError, StorageItem, StorageKey};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

fn open_store(tmp: &TempDir, name: &str) -> MdbxStore {
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: tmp.path().join(name),
        ..Default::default()
    });
    provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("mdbx store")
}

struct TestOverlay(Vec<(Vec<u8>, Option<Vec<u8>>)>);

#[test]
fn cursor_write_sampling_scales_only_the_sampled_suffix() {
    assert_eq!(estimate_cursor_write_us(0, 0, 0, 0, None), 0);
    assert_eq!(estimate_cursor_write_us(64, 12_345_000, 0, 0, None), 12_345);
    assert_eq!(
        estimate_cursor_write_us(320, 1_000_000, 0, 0, Some(1_000)),
        1_256
    );
    assert_eq!(
        estimate_cursor_write_us(321, 1_000_000, 256_000, 256, Some(2_000)),
        1_258
    );
}

impl RawOverlaySource for TestOverlay {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        self.0.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        for (key, value) in &self.0 {
            sink.visit(key, value.as_deref());
        }
    }
}

#[test]
fn provider_defaults_to_production_sized_tuning() {
    let provider = MdbxStoreProvider::new(StorageConfig::default());
    let tuning = provider.tuning();

    assert_eq!(tuning.map_size, 256 * 1024 * 1024 * 1024);
    assert_eq!(tuning.growth_step, 256 * 1024 * 1024);
    assert_eq!(tuning.max_readers, 4096);
}

#[test]
fn current_mdbx_wrapper_does_not_enforce_requested_max_readers() {
    let provider_source = include_str!("../../mdbx/provider.rs");
    let store_source = include_str!("../../mdbx/store.rs");

    assert!(
        provider_source.contains("does not enforce"),
        "provider docs must not claim mdbx_max_readers is enforced by the current wrapper"
    );
    assert!(
        store_source.contains("does not forward"),
        "store docs must keep the libmdbx max_readers limitation visible"
    );
    assert!(
        !store_source.contains("MDBX_opt_max_readers"),
        "current libmdbx adapter cannot claim direct max-reader enforcement"
    );
}

#[test]
fn opens_store_and_creates_environment_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("nested").join("mdbx");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    });

    let store = provider
        .get_store(std::path::Path::new(""))
        .expect("mdbx store");

    assert!(db_path.exists(), "MDBX environment directory should exist");
    let _snapshot = store.snapshot();
}

#[test]
fn named_table_views_isolate_identical_raw_keys() {
    let tmp = TempDir::new().expect("tempdir");
    let mut canonical = open_store(&tmp, "named-isolation");
    let mut state_service = canonical
        .open_named_table("neo_state_service")
        .expect("open StateService table");
    let key = b"same-key".to_vec();

    canonical
        .put(key.clone(), b"ledger".to_vec())
        .expect("write canonical value");
    state_service
        .put(key.clone(), b"state-root".to_vec())
        .expect("write StateService value");

    assert!(canonical.shares_environment_with(&state_service));
    assert_eq!(canonical.data_table_name(), None);
    assert_eq!(state_service.data_table_name(), Some("neo_state_service"));
    assert_eq!(canonical.try_get_bytes(&key), Some(b"ledger".to_vec()));
    assert_eq!(
        state_service.try_get_bytes(&key),
        Some(b"state-root".to_vec())
    );
}

#[test]
fn mdbx_exposes_the_static_coordinated_transaction_capability() {
    fn assert_capability<S: CoordinatedTransactionalStore>() {}
    assert_capability::<MdbxStore>();
}

#[test]
fn coordinated_overlays_publish_both_tables_in_one_transaction() {
    let tmp = TempDir::new().expect("tempdir");
    let canonical = open_store(&tmp, "coordinated");
    let state_service = canonical
        .open_named_table("neo_state_service")
        .expect("open StateService table");
    let canonical_key = b"canonical-tip".to_vec();
    let state_key = b"\x02".to_vec();
    let canonical_before = canonical.snapshot();
    let state_before = state_service.snapshot();
    let transaction_before = canonical.info().expect("MDBX info before").last_txnid();

    let mut canonical_overlay = TestOverlay(vec![(
        (canonical_key.clone()),
        Some(42u32.to_le_bytes().to_vec()),
    )]);
    let mut state_overlay = TestOverlay(vec![(
        (state_key.clone()),
        Some(42u32.to_le_bytes().to_vec()),
    )]);
    canonical
        .commit_coordinated_overlays(&mut canonical_overlay, &state_service, &mut state_overlay)
        .expect("coordinated commit");

    assert_eq!(canonical_before.try_get_bytes(&canonical_key), None);
    assert_eq!(state_before.try_get_bytes(&state_key), None);
    assert_eq!(
        canonical.try_get_bytes(&canonical_key),
        Some(42u32.to_le_bytes().to_vec())
    );
    assert_eq!(
        state_service.try_get_bytes(&state_key),
        Some(42u32.to_le_bytes().to_vec())
    );
    assert_eq!(
        canonical.info().expect("MDBX info after").last_txnid(),
        transaction_before + 1,
        "both overlays must cross one MDBX transaction boundary"
    );
}

#[test]
fn cursor_backed_raw_overlay_preserves_put_update_and_delete_semantics() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = open_store(&tmp, "cursor-overlay");
    store
        .put(b"update".to_vec(), b"old".to_vec())
        .expect("seed updated row");
    store
        .put(b"delete".to_vec(), b"old".to_vec())
        .expect("seed deleted row");

    let mut overlay = TestOverlay(vec![
        (b"delete".to_vec(), None),
        (b"missing".to_vec(), None),
        (b"insert".to_vec(), Some(b"new".to_vec())),
        (b"update".to_vec(), Some(b"new".to_vec())),
    ]);
    assert!(
        store
            .try_commit_borrowed_raw_overlay(&mut overlay)
            .expect("commit ordered cursor overlay")
    );

    assert_eq!(store.try_get_bytes(b"insert"), Some(b"new".to_vec()));
    assert_eq!(store.try_get_bytes(b"update"), Some(b"new".to_vec()));
    assert_eq!(store.try_get_bytes(b"delete"), None);
    assert_eq!(store.try_get_bytes(b"missing"), None);
}

#[test]
fn raw_overlay_commit_metrics_cover_phases_entries_and_bytes() {
    let before = MdbxCommitMetrics::snapshot();
    let tmp = TempDir::new().expect("tempdir");
    let store = open_store(&tmp, "commit-metrics");
    let mut overlay = TestOverlay(vec![
        (b"alpha".to_vec(), Some(b"one".to_vec())),
        (b"beta".to_vec(), Some(b"two".to_vec())),
    ]);

    store
        .commit_canonical_overlay(&mut overlay)
        .expect("instrumented raw overlay commit");

    let after = MdbxCommitMetrics::snapshot();
    assert!(after.stats.attempts > before.stats.attempts);
    assert!(after.stats.committed_transactions > before.stats.committed_transactions);
    assert!(after.stats.failures >= before.stats.failures);

    let stage_delta = |name: &str| {
        let before = before
            .stages
            .iter()
            .find(|stat| stat.stage == name)
            .map_or(0, |stat| stat.calls);
        after
            .stages
            .iter()
            .find(|stat| stat.stage == name)
            .map_or(0, |stat| stat.calls.saturating_sub(before))
    };
    for stage in [
        "total",
        "transaction_open",
        "table_open",
        "cursor_open",
        "overlay_visit",
        "cursor_write",
        "commit",
    ] {
        assert!(stage_delta(stage) >= 1, "missing stage metric {stage}");
    }

    let count_delta = |kind: &str| {
        let before = before
            .counts
            .iter()
            .find(|stat| stat.kind == kind)
            .map_or(0, |stat| stat.total);
        after
            .counts
            .iter()
            .find(|stat| stat.kind == kind)
            .map_or(0, |stat| stat.total.saturating_sub(before))
    };
    assert!(count_delta("entries") >= 2);
    assert!(count_delta("puts") >= 2);
    assert!(count_delta("key_bytes") >= 9);
    assert!(count_delta("value_bytes") >= 6);
}

#[test]
fn coordinated_commit_rejects_different_environments_without_partial_writes() {
    let tmp = TempDir::new().expect("tempdir");
    let canonical = open_store(&tmp, "coordinated-primary");
    let unrelated = open_store(&tmp, "coordinated-unrelated")
        .open_named_table("neo_state_service")
        .expect("open unrelated StateService table");
    let canonical_key = b"canonical-tip".to_vec();
    let state_key = b"\x02".to_vec();
    let mut canonical_overlay = TestOverlay(vec![(canonical_key.clone(), Some(vec![1]))]);
    let mut state_overlay = TestOverlay(vec![(state_key.clone(), Some(vec![1]))]);

    let error = canonical
        .commit_coordinated_overlays(&mut canonical_overlay, &unrelated, &mut state_overlay)
        .expect_err("different environments must be rejected");

    assert!(error.to_string().contains("same environment"));
    assert_eq!(canonical.try_get_bytes(&canonical_key), None);
    assert_eq!(unrelated.try_get_bytes(&state_key), None);
}

#[test]
fn coordinated_commit_rolls_back_primary_when_secondary_write_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: tmp.path().join("coordinated-rollback"),
        ..Default::default()
    })
    .with_map_size(4 * 1024 * 1024)
    .with_growth_step(1024 * 1024);
    let canonical = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("open constrained MDBX environment");
    let state_service = canonical
        .open_named_table("neo_state_service")
        .expect("open StateService table");
    let canonical_key = b"canonical-tip".to_vec();
    let state_key = b"large-state-node".to_vec();
    let mut canonical_overlay = TestOverlay(vec![(canonical_key.clone(), Some(vec![1]))]);
    let mut state_overlay = TestOverlay(vec![(
        state_key.clone(),
        Some(vec![0xAA; 16 * 1024 * 1024]),
    )]);

    canonical
        .commit_coordinated_overlays(&mut canonical_overlay, &state_service, &mut state_overlay)
        .expect_err("secondary write beyond map geometry must fail the transaction");

    assert_eq!(canonical.try_get_bytes(&canonical_key), None);
    assert_eq!(state_service.try_get_bytes(&state_key), None);
}

#[test]
fn named_table_validation_rejects_reserved_or_wrapper_panicking_names() {
    let tmp = TempDir::new().expect("tempdir");
    let canonical = open_store(&tmp, "named-validation");

    for invalid in ["", "neo_node_metadata", "state\0service"] {
        canonical
            .open_named_table(invalid)
            .expect_err("invalid named table must be rejected");
    }
}

#[test]
fn named_table_data_survives_environment_reopen() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("named-reopen");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });
    let key = b"\x02".to_vec();

    {
        let canonical = provider
            .get_mdbx_store(std::path::Path::new(""))
            .expect("open MDBX environment");
        let mut state_service = canonical
            .open_named_table("neo_state_service")
            .expect("open StateService table");
        state_service
            .put(key.clone(), 77u32.to_le_bytes().to_vec())
            .expect("write StateService height");
    }

    let canonical = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("reopen MDBX environment");
    let state_service = canonical
        .open_named_table("neo_state_service")
        .expect("reopen StateService table");
    assert_eq!(canonical.try_get_bytes(&key), None);
    assert_eq!(
        state_service.try_get_bytes(&key),
        Some(77u32.to_le_bytes().to_vec())
    );
}

#[test]
fn returns_error_when_path_is_file() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("not-a-dir");
    fs::write(&db_path, b"file").expect("write file");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });

    let err = match provider.get_store(std::path::Path::new("")) {
        Ok(_) => panic!("opening a regular file as an MDBX directory should fail"),
        Err(err) => err,
    };

    let message = err.to_string().to_ascii_lowercase();
    assert!(
        message.contains("failed to open mdbx store")
            || message.contains("failed to create mdbx data directory"),
        "unexpected error: {err}"
    );
}

#[test]
fn raw_prefix_find_returns_only_matching_rows_in_both_directions() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = open_store(&tmp, "prefix");
    for (key, value) in [
        (b"a\x00".to_vec(), vec![0x01]),
        (b"a\xff".to_vec(), vec![0x02]),
        (b"b".to_vec(), vec![0x03]),
    ] {
        store.put(key, value).expect("put raw row");
    }

    let prefix = b"a".to_vec();
    let forward_expected = vec![b"a\x00".to_vec(), b"a\xff".to_vec()];
    let backward_expected = vec![b"a\xff".to_vec(), b"a\x00".to_vec()];

    let store_forward_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_forward_keys, forward_expected);

    let store_backward_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_backward_keys, backward_expected);

    let snapshot = store.snapshot();
    let snapshot_forward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_forward_keys, forward_expected);

    let snapshot_backward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_backward_keys, backward_expected);
}

#[test]
fn snapshot_reads_ignore_pending_writes_until_reopened_after_commit() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = open_store(&tmp, "snapshot");
    let existing_key = b"k1".to_vec();
    let added_key = b"k2".to_vec();

    store
        .put(existing_key.clone(), vec![0xAA])
        .expect("put existing row");

    let mut snapshot = store.snapshot();
    {
        let snapshot_mut = Arc::get_mut(&mut snapshot).expect("exclusive snapshot");
        snapshot_mut.delete(existing_key.clone()).unwrap();
        snapshot_mut.put(added_key.clone(), vec![0xBB]).unwrap();
    }

    assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
    assert_eq!(
        snapshot.try_get_bytes(existing_key.as_slice()),
        Some(vec![0xAA])
    );
    assert_eq!(snapshot.try_get(&added_key), None);
    assert_eq!(snapshot.try_get_bytes(added_key.as_slice()), None);

    Arc::get_mut(&mut snapshot)
        .expect("exclusive snapshot")
        .try_commit()
        .expect("snapshot commit");

    assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
    assert_eq!(
        snapshot.try_get_bytes(existing_key.as_slice()),
        Some(vec![0xAA])
    );
    assert_eq!(snapshot.try_get(&added_key), None);
    assert_eq!(snapshot.try_get_bytes(added_key.as_slice()), None);

    let reopened = store.snapshot();
    assert_eq!(reopened.try_get(&existing_key), None);
    assert_eq!(reopened.try_get_bytes(existing_key.as_slice()), None);
    assert_eq!(reopened.try_get(&added_key), Some(vec![0xBB]));
    assert_eq!(
        reopened.try_get_bytes(added_key.as_slice()),
        Some(vec![0xBB])
    );
}

#[test]
fn raw_batch_reads_preserve_input_order_duplicates_and_snapshot_isolation() {
    let tmp = TempDir::new().expect("tempdir");
    let root = open_store(&tmp, "raw-batch-read");
    let mut store = root
        .open_named_table("raw-batch-read-table")
        .expect("open named batch-read table");
    let alpha = b"alpha".to_vec();
    let beta = b"beta".to_vec();
    let future = b"future".to_vec();
    let absent = b"absent".to_vec();

    store
        .put(alpha.clone(), b"alpha-old".to_vec())
        .expect("seed alpha");
    store
        .put(beta.clone(), b"beta-old".to_vec())
        .expect("seed beta");
    let snapshot = store.snapshot();

    store
        .put(alpha.clone(), b"alpha-new".to_vec())
        .expect("update alpha");
    store.delete(beta.clone()).expect("delete beta");
    store
        .put(future.clone(), b"future-new".to_vec())
        .expect("insert future");

    let keys = [
        beta.as_slice(),
        future.as_slice(),
        alpha.as_slice(),
        beta.as_slice(),
        absent.as_slice(),
    ];
    assert_eq!(
        snapshot
            .try_get_bytes_result(alpha.as_slice())
            .expect("snapshot point read"),
        Some(b"alpha-old".to_vec())
    );
    assert_eq!(
        snapshot
            .try_get_many_bytes(&keys)
            .expect("snapshot batch read"),
        vec![
            Some(b"beta-old".to_vec()),
            None,
            Some(b"alpha-old".to_vec()),
            Some(b"beta-old".to_vec()),
            None,
        ]
    );
    assert_eq!(
        store
            .try_get_many_bytes(&keys)
            .expect("live store batch read"),
        vec![
            None,
            Some(b"future-new".to_vec()),
            Some(b"alpha-new".to_vec()),
            None,
            None,
        ]
    );

    let after_batch_read = b"after-batch-read".to_vec();
    store
        .put(after_batch_read.clone(), b"visible".to_vec())
        .expect("insert after batch read");
    store
        .delete(future.clone())
        .expect("delete after batch read");
    assert_eq!(
        store
            .try_get_many_bytes(&[after_batch_read.as_slice(), future.as_slice()])
            .expect("subsequent batch read"),
        vec![Some(b"visible".to_vec()), None]
    );
}

#[test]
fn parallel_raw_batch_reads_preserve_order_and_frozen_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let root = open_store(&tmp, "parallel-raw-batch-read");
    let mut store = root
        .open_named_table("parallel-raw-batch-read-table")
        .expect("open named batch-read table");
    let mut seed = TestOverlay(
        (0u32..20_000)
            .map(|index| {
                (
                    index.to_be_bytes().to_vec(),
                    Some(index.to_le_bytes().to_vec()),
                )
            })
            .collect(),
    );
    assert!(
        store
            .try_commit_borrowed_raw_overlay(&mut seed)
            .expect("seed parallel batch rows")
    );
    let snapshot = store.snapshot();

    let mut keys = (0u32..20_000)
        .rev()
        .step_by(3)
        .map(|index| index.to_be_bytes().to_vec())
        .collect::<Vec<_>>();
    keys.push(10_000u32.to_be_bytes().to_vec());
    keys.push(u32::MAX.to_be_bytes().to_vec());

    let expected = keys
        .iter()
        .map(|key| {
            let index = u32::from_be_bytes(key.as_slice().try_into().unwrap());
            (index != u32::MAX).then(|| index.to_le_bytes().to_vec())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        snapshot
            .try_get_many_bytes_parallel_for_test(&keys, 4)
            .expect("four-reader frozen batch read"),
        expected
    );

    store
        .put(10_000u32.to_be_bytes().to_vec(), b"newer".to_vec())
        .expect("advance the live table after the snapshot");
    assert_eq!(
        snapshot
            .try_get_many_bytes_with_parallelism(&keys, 4)
            .expect("snapshot mismatch falls back to the frozen reader"),
        expected
    );
}

fn occupancy_test_key(bucket: u32, fill: u8) -> Vec<u8> {
    let mut key = vec![0xf0];
    key.extend_from_slice(&bucket.to_be_bytes());
    key.resize(33, fill);
    key
}

#[test]
fn prefix_occupancy_preserves_hits_collisions_order_and_runtime_updates() {
    let tmp = TempDir::new().expect("tempdir");
    let root = open_store(&tmp, "prefix-occupancy");
    let mut store = root
        .open_named_table("prefix-occupancy-table")
        .expect("open indexed table");
    let present = occupancy_test_key(0x1200_0000, 1);
    let colliding_absent = occupancy_test_key(0x12ff_ffff, 2);
    let definite_absent = occupancy_test_key(0x3400_0000, 3);
    let inserted = occupancy_test_key(0x5600_0000, 4);
    let ineligible = b"ordinary-key".to_vec();
    store
        .put(present.clone(), b"present".to_vec())
        .expect("seed indexed key");
    store
        .put(ineligible.clone(), b"ordinary".to_vec())
        .expect("seed ineligible key");
    let spec = PrefixOccupancySpec::new(
        Some("prefix-occupancy-table".to_string()),
        vec![0xf0],
        33,
        8,
    )
    .unwrap();
    store
        .install_prefix_occupancy_for_test(spec, std::slice::from_ref(&present))
        .unwrap();

    let snapshot = store.snapshot();
    assert_eq!(
        snapshot
            .try_get_many_bytes(&[
                definite_absent.as_slice(),
                present.as_slice(),
                colliding_absent.as_slice(),
                ineligible.as_slice(),
            ])
            .unwrap(),
        vec![
            None,
            Some(b"present".to_vec()),
            None,
            Some(b"ordinary".to_vec()),
        ]
    );

    store
        .put(inserted.clone(), b"inserted".to_vec())
        .expect("commit a post-baseline key");
    let snapshot = store.snapshot();
    assert_eq!(
        snapshot.try_get_many_bytes(&[inserted.as_slice()]).unwrap(),
        vec![Some(b"inserted".to_vec())]
    );

    store.delete(present.clone()).expect("delete indexed key");
    let snapshot = store.snapshot();
    assert_eq!(
        snapshot.try_get_many_bytes(&[present.as_slice()]).unwrap(),
        vec![None]
    );
}

#[test]
fn stale_prefix_occupancy_falls_back_after_an_unobserved_writer() {
    let tmp = TempDir::new().expect("tempdir");
    let root = open_store(&tmp, "stale-prefix-occupancy");
    let mut indexed = root
        .open_named_table("stale-prefix-table")
        .expect("open indexed table");
    let baseline = occupancy_test_key(0x1200_0000, 1);
    let later = occupancy_test_key(0x3400_0000, 2);
    indexed
        .put(baseline.clone(), b"baseline".to_vec())
        .expect("seed baseline key");
    let spec = PrefixOccupancySpec::new(Some("stale-prefix-table".to_string()), vec![0xf0], 33, 8)
        .unwrap();
    indexed
        .install_prefix_occupancy_for_test(spec, &[baseline])
        .unwrap();

    let mut unobserved = root
        .open_named_table("stale-prefix-table")
        .expect("open view without the process-local index");
    unobserved
        .put(later.clone(), b"later".to_vec())
        .expect("commit through unobserved writer");

    let snapshot = indexed.snapshot();
    assert_eq!(
        snapshot.try_get_many_bytes(&[later.as_slice()]).unwrap(),
        vec![Some(b"later".to_vec())],
        "coverage mismatch must restore authoritative MDBX reads"
    );
}

#[test]
fn fallible_raw_reads_reject_an_invalid_snapshot_without_changing_legacy_reads() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "invalid-snapshot-read"));
    let expected = StorageError::backend("injected snapshot initialization failure");
    let mut snapshot =
        MdbxSnapshot::with_initialization_error(Arc::clone(&store), expected.clone());

    assert_eq!(snapshot.try_get_bytes(b"key"), None);
    assert_eq!(snapshot.try_get_bytes_result(b"key"), Err(expected.clone()));
    assert_eq!(
        snapshot.try_get_many_bytes(&[b"key"]),
        Err(expected.clone())
    );
    assert_eq!(snapshot.try_get_many_bytes::<&[u8]>(&[]), Ok(Vec::new()));

    snapshot
        .put(b"unsafe".to_vec(), b"value".to_vec())
        .expect("stage write on invalid snapshot");
    assert_eq!(snapshot.try_commit(), Err(expected));
    assert_eq!(store.try_get_bytes(b"unsafe"), None);
}

#[test]
fn fallible_typed_point_reads_reject_an_invalid_snapshot_without_changing_legacy_reads() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "invalid-snapshot-typed-read"));
    let expected = StorageError::backend("injected snapshot initialization failure");
    let snapshot = MdbxSnapshot::with_initialization_error(Arc::clone(&store), expected.clone());
    let key = b"typed-key".to_vec();

    // Legacy Option-based reads remain soft-fail for compatibility.
    assert_eq!(snapshot.try_get(&key), None);
    // Fallible API must surface the backend failure so canonical callers can abort.
    assert_eq!(snapshot.try_get_result(&key), Err(expected));

    let mut runtime = RuntimeStore::Mdbx(open_store(&tmp, "runtime-typed-fallible-live"));
    let runtime_key = b"present".to_vec();
    runtime
        .put(runtime_key.clone(), b"value".to_vec())
        .expect("seed runtime store");
    assert_eq!(
        runtime
            .try_get_result(&runtime_key)
            .expect("runtime fallible get"),
        Some(b"value".to_vec())
    );
    assert_eq!(
        runtime
            .try_get_result(&b"missing".to_vec())
            .expect("missing key is Ok(None)"),
        None
    );
}

#[test]
fn runtime_raw_batch_reads_delegate_to_store_and_pinned_snapshot_backends() {
    let tmp = TempDir::new().expect("tempdir");
    let mut runtime = RuntimeStore::Mdbx(open_store(&tmp, "runtime-raw-batch-read"));
    let key = b"key".to_vec();
    let missing = b"missing".to_vec();

    runtime
        .put(key.clone(), b"old".to_vec())
        .expect("seed runtime store");
    let snapshot = runtime.snapshot();
    runtime
        .put(key.clone(), b"new".to_vec())
        .expect("update runtime store");
    let keys = [key.as_slice(), missing.as_slice(), key.as_slice()];

    assert_eq!(
        runtime
            .try_get_bytes_result(key.as_slice())
            .expect("runtime point read"),
        Some(b"new".to_vec())
    );
    assert_eq!(
        runtime
            .try_get_many_bytes(&keys)
            .expect("runtime store batch read"),
        vec![Some(b"new".to_vec()), None, Some(b"new".to_vec())]
    );
    assert_eq!(
        snapshot
            .try_get_many_bytes(&keys)
            .expect("runtime snapshot batch read"),
        vec![Some(b"old".to_vec()), None, Some(b"old".to_vec())]
    );
}

#[test]
fn store_cache_commits_mdbx_store_without_snapshot_overlay() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "store-cache"));

    let key_keep = StorageKey::new(42, vec![0x01]);
    let key_delete = StorageKey::new(42, vec![0x02]);
    let key_add = StorageKey::new(42, vec![0x03]);

    let mut seed = StoreCache::new_from_store(store.clone(), false);
    seed.add(key_keep.clone(), StorageItem::from_bytes(vec![0x10]));
    seed.add(key_delete.clone(), StorageItem::from_bytes(vec![0x20]));
    seed.try_commit().expect("seed MDBX store cache");

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.update(key_keep.clone(), StorageItem::from_bytes(vec![0x11]));
    writer.delete(key_delete.clone());
    writer.add(key_add.clone(), StorageItem::from_bytes(vec![0x30]));
    writer.try_commit().expect("store cache direct MDBX commit");
    writer
        .try_commit()
        .expect("second commit should be a no-op after cache is clean");

    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key_keep).map(|item| item.to_value()),
        Some(vec![0x11])
    );
    assert!(reader.get(&key_delete).is_none());
    assert_eq!(
        reader.get(&key_add).map(|item| item.to_value()),
        Some(vec![0x30])
    );
}

#[test]
fn snapshot_open_does_not_materialize_entire_mdbx_namespace() {
    let source = include_str!("../../mdbx/snapshot.rs");

    assert!(
        !source.contains("snapshot_entries"),
        "MDBX snapshots must use an MVCC read transaction instead of full keyspace materialization"
    );
    assert!(
        !source.contains("immutable_data"),
        "MDBX snapshots must not clone the full backend into memory"
    );
}

#[test]
fn backward_prefix_find_uses_reverse_cursor_without_forward_materialization() {
    let store_source = include_str!("../../mdbx/store.rs");
    let snapshot_source = include_str!("../../mdbx/snapshot.rs");

    for (name, source) in [("store", store_source), ("snapshot", snapshot_source)] {
        assert!(
            !source.contains("entries.reverse()"),
            "MDBX {name} backward scans must traverse the cursor backward instead of collecting forward rows"
        );
    }
}

#[test]
fn typed_prefix_find_matches_storage_key_rows() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "typed-prefix"));
    let key_a = StorageKey::new(-5, vec![0x1d, 0x00]);
    let key_b = StorageKey::new(-5, vec![0x1d, 0xff]);
    let key_other = StorageKey::new(-5, vec![0x1e, 0x00]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key_a.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(key_b.clone(), StorageItem::from_bytes(vec![0x02]));
    writer.add(key_other, StorageItem::from_bytes(vec![0x03]));
    writer.try_commit().expect("commit typed rows");

    let prefix = StorageKey::create(-5, 0x1d);
    let cache = StoreCache::new_from_store(store, false);
    let keys: Vec<_> = cache
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key.to_array())
        .collect();

    assert_eq!(keys, vec![key_b.to_array(), key_a.to_array()]);
}

#[test]
fn data_persists_after_reopen() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("persist");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });
    let key = b"persisted".to_vec();

    {
        let mut store = provider
            .get_mdbx_store(std::path::Path::new(""))
            .expect("open mdbx store");
        store
            .put(key.clone(), b"value".to_vec())
            .expect("write persisted row");
    }

    let reopened = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("reopen mdbx store");
    assert_eq!(reopened.try_get(&key), Some(b"value".to_vec()));
}

#[test]
fn maintenance_commit_is_atomic_and_metadata_is_not_in_the_data_table() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("maintenance");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });
    let data_key = StorageKey::new(-4, vec![9, 0, 0, 0, 1]).to_array();
    let metadata_key = b"ledger-pruned-through".to_vec();

    {
        let mut store = provider
            .get_mdbx_store(std::path::Path::new(""))
            .expect("open mdbx store");
        store
            .put(data_key.clone(), b"archived".to_vec())
            .expect("seed data row");

        let mut batch = StoreMaintenanceBatch::new();
        batch.delete_data(data_key.clone());
        batch.put_metadata(metadata_key.clone(), 42u32.to_be_bytes().to_vec());
        store
            .commit_maintenance(&batch)
            .expect("maintenance commit");

        assert_eq!(store.try_get_bytes(&data_key), None);
        assert_eq!(
            store
                .maintenance_metadata(&metadata_key)
                .expect("metadata read"),
            Some(42u32.to_be_bytes().to_vec())
        );
        assert!(
            <MdbxStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
                &store,
                None,
                SeekDirection::Forward,
            )
            .all(|(key, _)| key != metadata_key),
            "maintenance metadata must not enter the normal data table"
        );
    }

    let reopened = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("reopen mdbx store");
    assert_eq!(reopened.try_get_bytes(&data_key), None);
    assert_eq!(
        reopened
            .maintenance_metadata(&metadata_key)
            .expect("reopened metadata read"),
        Some(42u32.to_be_bytes().to_vec())
    );
}
