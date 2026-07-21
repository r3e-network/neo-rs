//! # Pack store tests
//!
//! Recovery, publication, compaction, lookup, lease, and format tests for the
//! append-only pack engine.
//!
//! ## Boundary
//!
//! These tests may inspect private store invariants but do not define runtime
//! APIs or persistent format behavior outside their fixtures.
//!
//! ## Contents
//!
//! - Core store behavior and subprocess lease coverage.
//! - Compaction and crash-recovery campaigns.
//! - Materialized-view evidence verification.

use super::*;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::process::{Command, Stdio};
use tempfile::tempdir;

const WRITER_LEASE_PROCESS_ROOT_ENV: &str = "NEO_STATE_PACKS_WRITER_LEASE_TEST_ROOT";
const WRITER_LEASE_PROCESS_READY: &str = "neo-state-packs-writer-lease-ready";

fn key(tag: u8) -> [u8; PACK_KEY_BYTES] {
    let mut key = [tag; PACK_KEY_BYTES];
    key[0] = TEST_NODE_PREFIX;
    key
}

fn numbered_key(number: u32) -> [u8; PACK_KEY_BYTES] {
    let mut key = [0; PACK_KEY_BYTES];
    key[0] = TEST_NODE_PREFIX;
    key[1..5].copy_from_slice(&number.to_be_bytes());
    key
}

const TEST_NODE_PREFIX: u8 = 0xf0;

fn put(key: [u8; PACK_KEY_BYTES], value: &[u8]) -> PackOperation {
    PackOperation {
        key,
        kind: PackOpKind::Put(value.to_vec()),
    }
}

fn tombstone(key: [u8; PACK_KEY_BYTES]) -> PackOperation {
    PackOperation {
        key,
        kind: PackOpKind::Tombstone,
    }
}

fn store_config(max_index_memory_bytes: u64) -> PackStoreConfig {
    PackStoreConfig::default()
        .with_max_index_memory_bytes(max_index_memory_bytes)
        .expect("valid test index-memory bound")
}

fn small_compaction_config(max_index_memory_bytes: u64) -> PackStoreConfig {
    store_config(max_index_memory_bytes)
        .with_compaction_bounds(2, 2, 3)
        .expect("valid small test compaction bounds")
}

fn append_without_maintenance(store: &mut PackStore, operations: &[PackOperation]) {
    let prepared = store
        .prepare_append(operations)
        .expect("prepare unmaintained frame");
    let sealed = store
        .seal_prepared(prepared)
        .expect("seal unmaintained frame");
    drop(sealed.into_snapshot());
}

fn assert_borrowed_frame_matches_owned_format_bytes(
    operations: &[PackOperation],
    with_prefix: bool,
) {
    let owned_root = tempdir().expect("temporary owned store");
    let borrowed_root = tempdir().expect("temporary borrowed store");

    let mut owned = PackStore::create(owned_root.path(), store_config(1024 * 1024))
        .expect("create owned-operation store");
    let mut borrowed = PackStore::create(borrowed_root.path(), store_config(1024 * 1024))
        .expect("create borrowed-operation store");
    if with_prefix {
        let prefix = [put(key(1), b"prefix")];
        append_without_maintenance(&mut owned, &prefix);
        append_without_maintenance(&mut borrowed, &prefix);
    }
    let owned_prepared = owned
        .prepare_append(operations)
        .expect("prepare owned frame");

    let mut builder = PackFrameBuilder::new(operations.len()).expect("create frame builder");
    for operation in operations {
        let value = match &operation.kind {
            PackOpKind::Put(value) => Some(value.as_slice()),
            PackOpKind::Tombstone => None,
        };
        builder
            .push(&operation.key, value)
            .expect("encode borrowed operation");
    }
    let borrowed_prepared = borrowed
        .prepare_built_append(builder)
        .expect("prepare borrowed frame");

    assert_eq!(borrowed_prepared.receipt(), owned_prepared.receipt());
    assert_eq!(
        fs::read(owned_root.path().join(PackSegmentId::INITIAL.file_name()),)
            .expect("read owned frame"),
        fs::read(
            borrowed_root
                .path()
                .join(PackSegmentId::INITIAL.file_name()),
        )
        .expect("read borrowed frame")
    );
    let epoch = u64::from(with_prefix);
    let run = run_file_name(0, epoch, epoch);
    assert_eq!(
        fs::read(owned_root.path().join("runs").join(&run)).expect("read owned run"),
        fs::read(borrowed_root.path().join("runs").join(&run)).expect("read borrowed run")
    );

    let owned_snapshot = owned
        .seal_prepared(owned_prepared)
        .expect("seal owned frame")
        .into_snapshot();
    let borrowed_snapshot = borrowed
        .seal_prepared(borrowed_prepared)
        .expect("seal borrowed frame")
        .into_snapshot();
    for operation in operations {
        let key = operation.key;
        assert_eq!(
            borrowed_snapshot.get(&key).expect("read borrowed value"),
            owned_snapshot.get(&key).expect("read owned value")
        );
    }
}

#[test]
fn borrowed_frame_builder_matches_owned_format_bytes() {
    let repeated = key(9);
    let deleted = key(2);
    let empty = key(7);
    let unsorted = vec![
        put(repeated, b"first"),
        tombstone(deleted),
        put(repeated, b"newest"),
        put(empty, b""),
    ];
    let sorted = vec![
        tombstone(deleted),
        put(empty, b""),
        put(repeated, b"first"),
        put(repeated, b"newest"),
    ];
    assert_borrowed_frame_matches_owned_format_bytes(&unsorted, false);
    assert_borrowed_frame_matches_owned_format_bytes(&sorted, true);
}

#[test]
fn borrowed_frame_builder_fails_before_writing_on_invalid_input() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create pack store");

    let mut incomplete = PackFrameBuilder::new(2).expect("create incomplete builder");
    incomplete
        .push(&key(1), Some(b"one"))
        .expect("encode first row");
    let error = store
        .prepare_built_append(incomplete)
        .expect_err("incomplete builder must fail");
    assert!(error.to_string().contains("encoded 1 rows, expected 2"));
    assert_eq!(
        fs::metadata(root.path().join(PackSegmentId::INITIAL.file_name()))
            .expect("stat untouched frame file")
            .len(),
        segment::SEGMENT_HEADER_LEN as u64
    );

    let mut wrong_value_bytes =
        PackFrameBuilder::with_value_bytes(1, 4).expect("create exact-size builder");
    wrong_value_bytes
        .push(&key(2), Some(b"one"))
        .expect("encode undersized value");
    let error = store
        .prepare_built_append(wrong_value_bytes)
        .expect_err("aggregate value-byte mismatch must fail");
    assert!(error.to_string().contains("payload bytes"));
    assert_eq!(
        fs::metadata(root.path().join(PackSegmentId::INITIAL.file_name()))
            .expect("stat untouched frame file")
            .len(),
        segment::SEGMENT_HEADER_LEN as u64
    );

    let mut invalid_key = PackFrameBuilder::new(1).expect("create invalid-key builder");
    let error = invalid_key
        .push(&[0u8; PACK_KEY_BYTES - 1], Some(b"value"))
        .expect_err("short key must fail");
    assert!(error.to_string().contains("expected 33"));
    assert!(invalid_key.is_empty());

    let mut excess_rows = PackFrameBuilder::new(1).expect("create bounded builder");
    excess_rows
        .push(&key(3), Some(b"one"))
        .expect("encode declared row");
    let error = excess_rows
        .push(&key(4), Some(b"two"))
        .expect_err("excess row must fail");
    assert!(
        error
            .to_string()
            .contains("more than its declared row count")
    );

    let mut undersized =
        PackFrameBuilder::with_value_bytes(1, 2).expect("create undersized builder");
    let error = undersized
        .push(&key(5), Some(b"three"))
        .expect_err("value above declared aggregate must fail");
    assert!(error.to_string().contains("declared aggregate byte count"));
    assert!(undersized.is_empty());

    let mut valid = PackFrameBuilder::new(1).expect("create replacement builder");
    valid
        .push(&key(3), Some(b"valid"))
        .expect("encode replacement row");
    store
        .prepare_built_append(valid)
        .expect("store remains usable after rejected builders");
}

#[test]
fn writer_lease_excludes_a_second_store_until_drop() {
    let root = tempdir().expect("temporary append store");
    let store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create first writer");

    let error = PackStore::open(root.path(), store_config(1024 * 1024))
        .err()
        .expect("second writer must be rejected");
    assert!(matches!(error, PackStoreError::WriterOwned { .. }));

    drop(store);
    PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("lease releases with writer drop");
}

#[test]
#[ignore = "subprocess worker selected by writer_lease_excludes_an_independent_process_until_exit"]
fn writer_lease_process_worker() {
    let root = std::env::var_os(WRITER_LEASE_PROCESS_ROOT_ENV)
        .map(PathBuf::from)
        .expect("writer-lease fixture root is set");
    let store =
        PackStore::open(&root, store_config(1024 * 1024)).expect("child acquires writer lease");

    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{WRITER_LEASE_PROCESS_READY}").expect("announce acquired lease");
    stdout.flush().expect("flush acquired-lease announcement");

    let mut release = [0u8; 1];
    std::io::stdin()
        .read_exact(&mut release)
        .expect("parent releases child writer");
    // Exit with the store still live so the coordinator proves the kernel
    // releases the lease even when Rust destructors do not run.
    std::mem::forget(store);
    std::process::exit(0);
}

#[test]
fn writer_lease_excludes_an_independent_process_until_exit() {
    let root = tempdir().expect("temporary append store");
    drop(PackStore::create(root.path(), store_config(1024 * 1024)).expect("create pack store"));

    let mut child = Command::new(std::env::current_exe().expect("resolve current test executable"))
        .arg("--ignored")
        .arg("--exact")
        .arg("engine::store::tests::writer_lease_process_worker")
        .arg("--test-threads=1")
        .arg("--nocapture")
        .env(WRITER_LEASE_PROCESS_ROOT_ENV, root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn independent writer process");
    let mut child_stdout = BufReader::new(child.stdout.take().expect("capture child stdout"));

    loop {
        let mut line = String::new();
        let bytes = child_stdout
            .read_line(&mut line)
            .expect("read child readiness announcement");
        assert_ne!(bytes, 0, "child exited before acquiring the writer lease");
        if line.contains(WRITER_LEASE_PROCESS_READY) {
            break;
        }
    }

    let competing_open = PackStore::open(root.path(), store_config(1024 * 1024));
    let excluded = matches!(competing_open, Err(PackStoreError::WriterOwned { .. }));

    let mut child_stdin = child.stdin.take().expect("open child release channel");
    let release_result = child_stdin.write_all(b"x");
    drop(child_stdin);
    let status = child.wait().expect("wait for independent writer process");
    let mut child_output = String::new();
    child_stdout
        .read_to_string(&mut child_output)
        .expect("read remaining child stdout");
    let mut child_error = String::new();
    child
        .stderr
        .take()
        .expect("capture child stderr")
        .read_to_string(&mut child_error)
        .expect("read child stderr");

    assert!(
        release_result.is_ok(),
        "release independent writer process: {release_result:?}"
    );
    assert!(
        status.success(),
        "independent writer process failed with {status}\nstdout:\n{child_output}\nstderr:\n{child_error}"
    );
    assert!(
        excluded,
        "independent writer must own the lease exclusively"
    );
    PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("lease releases after independent writer termination");
}

#[cfg(unix)]
#[test]
fn writer_lease_canonicalizes_a_symlinked_store_path() {
    let parent = tempdir().expect("temporary pack parent");
    let root = parent.path().join("store");
    let alias = parent.path().join("store-alias");
    let store = PackStore::create(&root, store_config(1024 * 1024)).expect("create first writer");
    std::os::unix::fs::symlink(&root, &alias).expect("create store symlink");

    let error = PackStore::open(&alias, store_config(1024 * 1024))
        .err()
        .expect("symlink alias must share the writer lease");
    assert!(matches!(error, PackStoreError::WriterOwned { .. }));

    drop(store);
    PackStore::open(&alias, store_config(1024 * 1024)).expect("alias opens after lease release");
}

#[test]
fn frame_header_enforces_row_payload_and_allocation_hard_limits() {
    let checksum = [0u8; 32];
    let error =
        encode_frame_header(0, 0, 0, checksum).expect_err("empty frame header must be rejected");
    assert!(error.to_string().contains("at least one row"));

    let too_many_rows = usize::try_from(MAX_FRAME_ROWS + 1).expect("row limit fits usize");
    let error = encode_frame_header(
        0,
        too_many_rows,
        usize::try_from(MAX_FRAME_PAYLOAD_BYTES).expect("payload limit fits usize"),
        checksum,
    )
    .expect_err("oversized row count must be rejected");
    assert!(error.to_string().contains("row count exceeds"));

    let error = encode_frame_header(
        0,
        1,
        usize::try_from(MAX_FRAME_PAYLOAD_BYTES + 1).expect("payload overflow fits usize"),
        checksum,
    )
    .expect_err("oversized payload must be rejected");
    assert!(error.to_string().contains("payload exceeds"));

    let short_payload =
        usize::try_from(2 * FRAME_ROW_HEADER_BYTES - 1).expect("short payload length fits usize");
    let error = encode_frame_header(0, 2, short_payload, checksum)
        .expect_err("short row payload must be rejected");
    assert!(error.to_string().contains("too short"));

    let mut malicious = [0u8; FRAME_HEADER_LEN];
    malicious[0..8].copy_from_slice(FRAME_MAGIC);
    malicious[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    malicious[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
    malicious[24..32].copy_from_slice(&1u64.to_le_bytes());
    malicious[32..40].copy_from_slice(&(MAX_FRAME_PAYLOAD_BYTES + 1).to_le_bytes());
    let error = validate_frame_header(&malicious, 0)
        .expect_err("oversized reopened payload must be rejected");
    assert!(error.to_string().contains("payload exceeds"));

    malicious[8..12].copy_from_slice(&(PACK_FRAME_FORMAT_VERSION + 1).to_le_bytes());
    malicious[32..40].copy_from_slice(&FRAME_ROW_HEADER_BYTES.to_le_bytes());
    let error =
        validate_frame_header(&malicious, 0).expect_err("unknown frame version must fail closed");
    assert!(matches!(
        error.downcast_ref::<PackStoreError>(),
        Some(PackStoreError::UnsupportedVersion {
            artifact: PackStoreArtifact::Frame,
            found,
            ..
        }) if *found == PACK_FRAME_FORMAT_VERSION + 1
    ));
}

#[test]
fn random_point_mmaps_are_opt_in_and_survive_append_compaction_and_reopen() {
    let default_root = tempdir().expect("temporary default store");
    let mut default_store = PackStore::create(default_root.path(), store_config(1024 * 1024))
        .expect("create default store");
    default_store
        .append(&[put(key(1), b"default")])
        .expect("append through default mappings");
    assert!(default_store.lookup_pack_map.is_none());
    assert!(
        default_store
            .runs
            .iter()
            .all(|live| live.run.lookup_map.is_none())
    );

    let root = tempdir().expect("temporary random-mmap store");
    let options = PackStoreOptions {
        random_point_mmap: true,
        ..PackStoreOptions::default()
    };
    let config = small_compaction_config(1024 * 1024)
        .with_read_options(options)
        .expect("valid random-mmap test options");
    let mut store = PackStore::create(root.path(), config).expect("create random-mmap store");
    let first = key(10);
    let second = key(20);
    store
        .append(&[put(first, b"old"), put(second, b"present")])
        .expect("append initial versions");
    let pinned = store.snapshot().expect("pin initial generation");
    store
        .append(&[put(first, b"new")])
        .expect("append replacement");
    store
        .append(&[tombstone(second)])
        .expect("append tombstone and compact L0");

    assert!(store.lookup_pack_map.is_some());
    assert!(store.runs.iter().all(|live| live.run.lookup_map.is_some()));
    assert_eq!(store.runs.len(), 1);
    assert_eq!(store.runs[0].level, 1);
    assert_eq!(
        store.get(&first).expect("point replacement"),
        Some(b"new".to_vec())
    );
    assert_eq!(store.get(&second).expect("point tombstone"), None);
    assert_eq!(
        store
            .get_many_sorted(&[first, second])
            .expect("sorted current generation"),
        vec![Some(b"new".to_vec()), None]
    );
    assert_eq!(
        pinned.get(&first).expect("pinned point"),
        Some(b"old".to_vec())
    );
    assert_eq!(
        pinned
            .get_many_sorted(&[first, second])
            .expect("pinned sorted generation"),
        vec![Some(b"old".to_vec()), Some(b"present".to_vec())]
    );
    drop(pinned);
    drop(store);

    let reopened = PackStore::open(root.path(), config).expect("reopen with random mappings");
    assert!(reopened.lookup_pack_map.is_some());
    assert!(
        reopened
            .runs
            .iter()
            .all(|live| live.run.lookup_map.is_some())
    );
    assert_eq!(
        reopened.get(&first).expect("reopened point"),
        Some(b"new".to_vec())
    );
    assert_eq!(
        reopened
            .get_many_sorted(&[first, second])
            .expect("reopened sorted batch"),
        vec![Some(b"new".to_vec()), None]
    );
    let scrub = reopened
        .scrub_committed_frames()
        .expect("scrub normal mapping");
    assert_eq!(scrub.frames, 3);
    assert_eq!(scrub.tombstones, 1);
}

#[test]
fn pack_read_workers_are_bounded_and_parallel_batches_preserve_order() {
    for invalid in [0, 9] {
        let error = store_config(1024 * 1024)
            .with_read_options(PackStoreOptions {
                batch_value_workers: invalid,
                ..PackStoreOptions::default()
            })
            .expect_err("invalid worker count must fail before store creation");
        assert!(error.to_string().contains("workers must be in 1..=8"));
    }

    let root = tempdir().expect("temporary parallel-read store");
    let options = PackStoreOptions {
        random_point_mmap: true,
        batch_value_workers: 4,
    };
    let config = store_config(8 * 1024 * 1024)
        .with_read_options(options)
        .expect("valid parallel-read test options");
    let mut store = PackStore::create(root.path(), config).expect("create parallel-read store");
    let stored_keys = (0..1_028).map(numbered_key).collect::<Vec<_>>();
    let mut operations = stored_keys
        .iter()
        .enumerate()
        .rev()
        .map(|(index, key)| put(*key, &(index as u32).to_le_bytes()))
        .collect::<Vec<_>>();
    let deleted = numbered_key(1_028);
    let missing = numbered_key(1_029);
    operations.push(tombstone(deleted));
    store.append(&operations).expect("append shuffled values");

    let duplicate = numbered_key(512);
    let mut keys = stored_keys.clone();
    keys.push(duplicate);
    keys.push(deleted);
    keys.push(missing);
    keys.sort_unstable();
    let expected = keys
        .iter()
        .map(|key| {
            let number = u32::from_be_bytes(key[1..5].try_into().expect("numbered key"));
            (number < 1_028).then(|| number.to_le_bytes().to_vec())
        })
        .collect::<Vec<_>>();
    let returned_bytes = 1_029 * 4;

    assert_eq!(
        store
            .get_many_sorted_bounded(&keys, 4, returned_bytes)
            .expect("parallel bounded batch"),
        expected
    );
    let error = store
        .get_many_sorted_bounded(&keys, 4, returned_bytes - 1)
        .expect_err("parallel batch must enforce its aggregate byte bound");
    assert!(error.to_string().contains("exceeding the configured limit"));
    let pinned = store.snapshot().expect("pin parallel read generation");
    assert_eq!(
        pinned
            .get_many_sorted(&keys)
            .expect("parallel snapshot batch"),
        expected
    );
    drop(pinned);
    drop(store);

    let reopened = PackStore::open(root.path(), config).expect("reopen parallel-read store");
    assert_eq!(
        reopened
            .get_many_sorted(&keys)
            .expect("parallel reopened batch"),
        expected
    );
}

#[test]
fn newest_row_and_run_win_and_tombstones_survive_reopen() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(1);
    let second = key(2);

    store
        .append(&[
            put(first, b"old"),
            put(second, b"second"),
            put(first, b"same-frame-new"),
        ])
        .expect("append first frame");
    assert_eq!(
        store.get(&first).expect("read same-frame version"),
        Some(b"same-frame-new".to_vec())
    );
    store
        .append(&[put(first, b"new-run")])
        .expect("append replacement frame");
    assert_eq!(
        store.get(&first).expect("read newer run"),
        Some(b"new-run".to_vec())
    );
    let sorted = store
        .get_many_sorted(&[first, second])
        .expect("read sorted keys");
    assert_eq!(
        sorted,
        vec![Some(b"new-run".to_vec()), Some(b"second".to_vec())]
    );
    store.append(&[tombstone(first)]).expect("append tombstone");
    assert_eq!(store.get(&first).expect("read tombstone"), None);
    drop(store);

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024)).expect("reopen store");
    assert_eq!(reopened.get(&first).expect("read reopened tombstone"), None);
    assert_eq!(
        reopened.get(&second).expect("read reopened value"),
        Some(b"second".to_vec())
    );
    assert_eq!(reopened.open_validation().frames, 3);
    assert_eq!(reopened.open_validation().runs, 3);
}

#[test]
fn sorted_batch_restores_key_order_after_payload_offset_reads() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(0x11);
    let second = key(0x22);
    let third = key(0x33);
    let missing = key(0x44);

    // Payload offsets follow operation order, deliberately opposing the
    // sorted query order used by the index scan.
    store
        .append(&[
            put(third, b"third"),
            put(second, b"second"),
            put(first, b"first"),
        ])
        .expect("append reverse-offset values");
    store
        .append(&[tombstone(second)])
        .expect("append second-key tombstone");

    assert_eq!(
        store
            .get_many_sorted(&[first, first, second, third, missing])
            .expect("read reordered batch"),
        vec![
            Some(b"first".to_vec()),
            Some(b"first".to_vec()),
            None,
            Some(b"third".to_vec()),
            None,
        ]
    );
    assert!(
        store
            .get_many_sorted(&[third, first])
            .expect_err("unsorted batch must fail")
            .to_string()
            .contains("sorted")
    );
}

#[test]
fn bounded_reads_reject_indexed_values_before_result_allocation() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(0x11);
    let second = key(0x22);
    store
        .append(&[put(first, b"first"), put(second, b"second")])
        .expect("append bounded-read values");

    let point_error = store
        .get_bounded(&first, 4)
        .expect_err("oversized point result must fail before allocation");
    assert!(point_error.to_string().contains("indexed value length 5"));

    let batch_error = store
        .get_many_sorted_bounded(&[first, second], 6, 10)
        .expect_err("oversized aggregate result must fail before allocation");
    assert!(batch_error.to_string().contains("require 11 bytes"));

    assert_eq!(
        store
            .get_many_sorted_bounded(&[first, second], 6, 11)
            .expect("bounded batch at its exact limit"),
        vec![Some(b"first".to_vec()), Some(b"second".to_vec())]
    );
}

#[test]
fn index_scrub_rejects_payload_ranges_beyond_the_committed_pack() {
    let outside = IndexEntry {
        key: key(0x33),
        sequence: 0,
        value_offset: 100,
        value_len: 2,
        tombstone: false,
    };
    let error = validate_index_entry_payload_range(&outside, 101)
        .expect_err("out-of-pack index range must fail scrub");
    assert!(error.to_string().contains("beyond the committed pack"));

    let overflowing = IndexEntry {
        value_offset: u64::MAX,
        value_len: 1,
        ..outside
    };
    let error = validate_index_entry_payload_range(&overflowing, u64::MAX)
        .expect_err("overflowing index range must fail scrub");
    assert!(error.to_string().contains("overflows"));

    let tombstone_outside = IndexEntry {
        value_offset: 102,
        value_len: 0,
        tombstone: true,
        ..outside
    };
    let error = validate_index_entry_payload_range(&tombstone_outside, 101)
        .expect_err("out-of-pack tombstone offset must fail scrub");
    assert!(error.to_string().contains("beyond the committed pack"));
}

#[test]
fn append_rejects_decoded_index_memory_overflow_before_writing() {
    let root = tempdir().expect("temporary append store");
    let bound = std::mem::size_of::<IndexEntry>() as u64 - 1;
    let mut store =
        PackStore::create(root.path(), store_config(bound)).expect("create bounded store");
    let error = store
        .append(&[put(key(3), b"value")])
        .expect_err("index memory bound must reject frame");
    assert!(error.to_string().contains("exceeds configured bound"));
    assert_eq!(
        store.layout().expect("empty layout"),
        (segment::SEGMENT_HEADER_LEN as u64, 0, 0, 0),
    );
}

#[test]
fn reopen_rejects_corrupt_committed_frame_payload() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store
        .append(&[put(key(4), b"checksum-target")])
        .expect("append frame");
    drop(store);

    let pack_path = root.path().join(PackSegmentId::INITIAL.file_name());
    let mut pack = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pack_path)
        .expect("open pack for corruption");
    pack.seek(SeekFrom::Start(
        segment::SEGMENT_HEADER_LEN as u64 + FRAME_HEADER_LEN as u64 + 1,
    ))
    .expect("seek into payload");
    pack.write_all(&[0xff]).expect("corrupt payload");
    pack.sync_all().expect("sync corruption");
    drop(pack);

    let error = PackStore::open(root.path(), store_config(1024 * 1024))
        .err()
        .expect("corrupt frame must fail reopen");
    assert!(format!("{error:#}").contains("checksum mismatch"));
}

fn assert_corrupt_index_structure_is_rebuilt(byte_offset: u64) {
    let root = tempdir().expect("temporary append store");
    let target = key(5);
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store
        .append(&[put(target, b"structure-target")])
        .expect("append frame");
    let generation = store.snapshot().expect("snapshot").generation();
    drop(store);

    let run_path = root.path().join("runs").join(run_file_name(0, 0, 0));
    let mut run = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&run_path)
        .expect("open run for corruption");
    run.seek(SeekFrom::Start(byte_offset))
        .expect("seek to index structure byte");
    let mut byte = [0u8; 1];
    run.read_exact(&mut byte)
        .expect("read index structure byte");
    run.seek(SeekFrom::Start(byte_offset))
        .expect("rewind to index structure byte");
    run.write_all(&[byte[0] ^ 0x80])
        .expect("corrupt index structure byte");
    run.sync_all().expect("sync index structure corruption");
    drop(run);

    let error = read_index_run(&run_path).expect_err("structure corruption must be detected");
    assert!(error.to_string().contains("structure checksum mismatch"));

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("corrupt derived run must rebuild from its committed frame");
    assert_eq!(
        reopened.get(&target).expect("read rebuilt value"),
        Some(b"structure-target".to_vec())
    );
    assert!(
        reopened.snapshot().expect("rebuilt snapshot").generation() > generation,
        "recovery must publish a new manifest generation"
    );
    assert_eq!(
        reopened.metrics().expect("rebuild metrics").rebuild_frames,
        1,
        "recovery rebuild must report the committed frame prefix"
    );
}

#[test]
fn reopen_detects_and_rebuilds_corrupt_index_fence() {
    assert_corrupt_index_structure_is_rebuilt(INDEX_HEADER_LEN as u64);
}

#[test]
fn reopen_detects_and_rebuilds_corrupt_index_filter() {
    assert_corrupt_index_structure_is_rebuilt((INDEX_HEADER_LEN + FENCE_KEY_BYTES) as u64);
}

#[test]
fn legacy_v2_index_run_is_rebuilt_before_use() {
    let root = tempdir().expect("temporary append store");
    let target = key(6);
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store
        .append(&[put(target, b"legacy-index-target")])
        .expect("append frame");
    let generation = store.snapshot().expect("snapshot").generation();
    drop(store);

    let run_path = root.path().join("runs").join(run_file_name(0, 0, 0));
    let mut bytes = fs::read(&run_path).expect("read index run");
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    let tag = digest(&bytes[..INDEX_HEADER_TAG_START]);
    bytes[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN].copy_from_slice(&tag[..4]);
    fs::write(&run_path, bytes).expect("write legacy index version");

    let error = read_index_run(&run_path).expect_err("v2 run must require a rebuild");
    assert!(matches!(
        error.downcast_ref::<PackStoreError>(),
        Some(PackStoreError::UnsupportedVersion {
            artifact: PackStoreArtifact::IndexRun,
            found: 2,
            ..
        })
    ));

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("legacy derived run must rebuild from its committed frame");
    assert_eq!(
        reopened.get(&target).expect("read rebuilt value"),
        Some(b"legacy-index-target".to_vec())
    );
    assert!(
        reopened.snapshot().expect("rebuilt snapshot").generation() > generation,
        "migration must publish a new manifest generation"
    );
    let rebuilt = fs::read(&run_path).expect("read rebuilt index run");
    assert_eq!(
        u32_at(&rebuilt, 8).expect("read rebuilt index version"),
        PACK_INDEX_FORMAT_VERSION
    );
}

#[test]
fn committed_frame_scrub_counts_every_historical_row() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(41);
    let second = key(42);
    store
        .append(&[put(first, b"old"), put(second, b"second")])
        .expect("append first frame");
    store
        .append(&[put(first, b"new"), tombstone(second)])
        .expect("append second frame");
    let generation_before = store
        .snapshot()
        .expect("snapshot before republish")
        .generation();
    store
        .republish_manifest()
        .expect("republish unchanged manifest");
    let generation_after = store
        .snapshot()
        .expect("snapshot after republish")
        .generation();
    assert_eq!(generation_after, generation_before + 1);

    let scrub = store.scrub_committed_frames().expect("scrub frames");
    assert_eq!(scrub.frames, 2);
    assert_eq!(scrub.rows, 4);
    assert_eq!(scrub.puts, 3);
    assert_eq!(scrub.tombstones, 1);
    assert_eq!(scrub.value_bytes, 3 + 6 + 3);
    assert!(scrub.payload_bytes > scrub.value_bytes);
}

#[test]
fn checkpoint_scrub_hashes_every_ordered_key_and_value() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(45);
    let second = key(46);
    store
        .append(&[put(first, b"first")])
        .expect("append first checkpoint frame");
    store
        .append(&[put(second, b"second")])
        .expect("append second checkpoint frame");

    let evidence = store
        .scrub_checkpoint_namespace()
        .expect("scrub checkpoint namespace");
    let mut expected = Sha256::new();
    expected.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
    for (key, value) in [(first, b"first".as_slice()), (second, b"second".as_slice())] {
        expected.update((PACK_KEY_BYTES as u32).to_le_bytes());
        expected.update(key);
        expected.update((value.len() as u64).to_le_bytes());
        expected.update(value);
    }
    assert_eq!(evidence.sha256, <[u8; 32]>::from(expected.finalize()));
    assert_eq!(evidence.scrub.frames, 2);
    assert_eq!(evidence.scrub.rows, 2);
    assert_eq!(evidence.scrub.puts, 2);
    assert_eq!(evidence.scrub.tombstones, 0);
    assert_eq!(evidence.scrub.value_bytes, 11);
}

#[test]
fn checkpoint_scrub_rejects_versioned_or_tombstoned_streams() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let repeated = key(47);
    store
        .append(&[put(repeated, b"first")])
        .expect("append first version");
    store
        .append(&[put(repeated, b"second")])
        .expect("append repeated version");
    let error = store
        .scrub_checkpoint_namespace()
        .expect_err("checkpoint scrub must reject repeated keys");
    assert!(error.to_string().contains("not strictly increasing"));

    let tombstone_root = tempdir().expect("temporary tombstone store");
    let mut tombstone_store =
        PackStore::create(tombstone_root.path(), store_config(1024 * 1024)).expect("create store");
    tombstone_store
        .append(&[tombstone(key(48))])
        .expect("append tombstone");
    let error = tombstone_store
        .scrub_checkpoint_namespace()
        .expect_err("checkpoint scrub must reject tombstones");
    assert!(error.to_string().contains("contains a tombstone"));
}

#[test]
fn committed_frame_scrub_detects_corrupt_non_tail_payload() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store
        .append(&[put(key(43), b"first")])
        .expect("append first frame");
    store
        .append(&[put(key(44), b"tail")])
        .expect("append tail frame");
    drop(store);

    let pack_path = root.path().join(PackSegmentId::INITIAL.file_name());
    let mut pack = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pack_path)
        .expect("open pack for corruption");
    pack.seek(SeekFrom::Start(
        segment::SEGMENT_HEADER_LEN as u64 + FRAME_HEADER_LEN as u64 + FRAME_ROW_HEADER_BYTES,
    ))
    .expect("seek into first value");
    pack.write_all(&[0xff]).expect("corrupt first payload");
    pack.sync_all().expect("sync corruption");
    drop(pack);

    let error = PackStore::open(root.path(), store_config(1024 * 1024))
        .err()
        .expect("normal open must reject the older corrupt frame");
    assert!(
        format!("{error:#}").contains("frame 0"),
        "unexpected corruption error: {error:#}"
    );
}

#[test]
fn fence_probe_handles_boundaries_and_truncated_prefix_collisions() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    // 200 keys sharing the first 25 bytes: every fence prefix collides,
    // so probes widen to the whole run and spill off the stack buffer.
    // Even ordinals leave in-range gaps for absent-key probes.
    let mut keys = Vec::new();
    for ordinal in (0u64..400).step_by(2) {
        let mut key = [0xAAu8; PACK_KEY_BYTES];
        key[0] = TEST_NODE_PREFIX;
        key[25..33].copy_from_slice(&ordinal.to_be_bytes());
        keys.push(key);
    }
    let operations: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| put(*key, format!("value-{index}").as_bytes()))
        .collect();
    store.append(&operations).expect("append adversarial frame");
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(
            store.get(key).expect("read boundary key"),
            Some(format!("value-{index}").into_bytes())
        );
    }
    for ordinal in (1u64..400).step_by(2) {
        let mut absent = [0xAAu8; PACK_KEY_BYTES];
        absent[0] = TEST_NODE_PREFIX;
        absent[25..33].copy_from_slice(&ordinal.to_be_bytes());
        assert_eq!(store.get(&absent).expect("read absent in-range key"), None);
    }
    let mut below = [0xAAu8; PACK_KEY_BYTES];
    below[0] = 0x10;
    assert_eq!(store.get(&below).expect("read below-range key"), None);
    let mut above = [0xAAu8; PACK_KEY_BYTES];
    above[0] = TEST_NODE_PREFIX;
    above[25..33].copy_from_slice(&10_000u64.to_be_bytes());
    assert_eq!(store.get(&above).expect("read above-range key"), None);
    let sorted = store
        .get_many_sorted(&keys)
        .expect("batch read boundary keys");
    for (index, value) in sorted.iter().enumerate() {
        assert_eq!(value.as_deref(), Some(format!("value-{index}").as_bytes()));
    }
    drop(store);

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024)).expect("reopen store");
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(
            reopened.get(key).expect("read reopened boundary key"),
            Some(format!("value-{index}").into_bytes())
        );
    }
}

#[test]
fn reopen_truncates_torn_tail_without_a_published_run() {
    let root = tempdir().expect("temporary append store");
    let first = key(1);
    let second = key(2);
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    store
        .append(&[put(first, b"one")])
        .expect("append first frame");
    store
        .append(&[put(second, b"two")])
        .expect("append second frame");
    let (committed_len, _, _, _) = store.layout().expect("committed layout");
    drop(store);

    // Case one: torn partial frame header at the tail.
    let pack_path = root.path().join(PackSegmentId::INITIAL.file_name());
    let mut pack = OpenOptions::new()
        .append(true)
        .open(&pack_path)
        .expect("open pack for torn header");
    pack.write_all(&[0xABu8; 50]).expect("write torn header");
    pack.sync_all().expect("sync torn header");
    drop(pack);

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("reopen truncates a torn partial header");
    assert_eq!(reopened.open_validation().frames, 2);
    assert_eq!(
        reopened.get(&first).expect("read first after truncation"),
        Some(b"one".to_vec())
    );
    assert_eq!(
        reopened.get(&second).expect("read second after truncation"),
        Some(b"two".to_vec())
    );
    assert_eq!(
        reopened.layout().expect("truncated layout").0,
        committed_len
    );
    drop(reopened);

    // Case two: well-formed header whose payload is torn.
    let mut pack = OpenOptions::new()
        .append(true)
        .open(&pack_path)
        .expect("open pack for torn payload");
    let mut fake = [0u8; FRAME_HEADER_LEN];
    fake[0..8].copy_from_slice(FRAME_MAGIC);
    fake[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    fake[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
    fake[16..24].copy_from_slice(&2u64.to_le_bytes());
    fake[24..32].copy_from_slice(&1u64.to_le_bytes());
    fake[32..40].copy_from_slice(&1_000_000u64.to_le_bytes());
    pack.write_all(&fake).expect("write torn frame header");
    pack.write_all(&[0xCDu8; 128])
        .expect("write torn payload bytes");
    pack.sync_all().expect("sync torn payload");
    drop(pack);

    let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
        .expect("reopen truncates a torn payload");
    assert_eq!(reopened.open_validation().frames, 2);
    assert_eq!(
        reopened
            .get(&second)
            .expect("read second after payload truncation"),
        Some(b"two".to_vec())
    );
    assert_eq!(
        reopened.layout().expect("truncated layout").0,
        committed_len
    );
}

#[test]
fn streaming_compaction_workspace_fits_mainnet_scale_under_one_gibibyte() {
    let estimated = estimate_compaction_workspace(580 * 1024 * 1024, 224_024_920);
    assert!(
        estimated < 1024 * 1024 * 1024,
        "streaming MainNet compaction must fit the declared 1 GiB bound: {estimated}"
    );
    assert!(
        estimated > 768 * 1024 * 1024,
        "the estimate must include pinned v3 metadata and v4 output state: {estimated}"
    );
    let error = ensure_compaction_workspace(estimated, 768 * 1024 * 1024)
        .expect_err("MainNet-scale streaming build must honor a smaller bound");
    assert!(matches!(
        error.downcast_ref::<PackStoreError>(),
        Some(PackStoreError::CompactionWorkspaceExceeded {
            estimated_bytes,
            max_bytes,
        }) if *estimated_bytes == estimated && *max_bytes == 768 * 1024 * 1024
    ));
}

#[test]
fn compaction_workspace_preflight_fails_before_creating_output() {
    let root = tempdir().expect("temporary append store");
    let runs_dir = root.path().join("runs");
    let mut store =
        PackStore::create(root.path(), small_compaction_config(1024 * 1024)).expect("create store");
    append_without_maintenance(&mut store, &[put(key(1), b"v1")]);
    append_without_maintenance(&mut store, &[put(key(2), b"v2")]);
    append_without_maintenance(&mut store, &[put(key(3), b"v3")]);
    let mut plan = store
        .plan_compaction()
        .expect("plan compaction")
        .expect("overfull L0 has a plan");
    let estimated = plan.estimated_workspace_bytes();
    plan.max_index_memory_bytes = estimated - 1;
    let output_name = run_file_name(1, 0, 2);
    let output = runs_dir.join(&output_name);
    let temporary = runs_dir.join(format!("{output_name}.tmp"));

    let error = plan
        .build()
        .err()
        .expect("over-budget compaction must be deferred");
    assert!(matches!(
        error.downcast_ref::<PackStoreError>(),
        Some(PackStoreError::CompactionWorkspaceExceeded { .. })
    ));
    assert!(
        !output.exists(),
        "preflight must precede output publication"
    );
    assert!(
        !temporary.exists(),
        "preflight must precede temporary-file creation"
    );
    assert_eq!(
        store.get(&key(2)).expect("read source generation"),
        Some(b"v2".to_vec())
    );
}

#[test]
fn compaction_dedups_and_pinned_generation_reads_older_versions() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), small_compaction_config(1024 * 1024)).expect("create store");
    let target = key(1);
    store
        .append(&[put(target, b"v1"), put(key(2), b"a")])
        .expect("append frame 0");
    store.append(&[put(target, b"v2")]).expect("append frame 1");
    let pinned = store.snapshot().expect("pin generation 2");
    assert_eq!(pinned.generation(), 2);

    // Frame 2 pushes L0 past its bound: the first compaction cycle merges
    // all three frames into one L1 run, keeping the newest version.
    store.append(&[put(target, b"v3")]).expect("append frame 2");
    assert_eq!(store.runs.len(), 1);
    assert_eq!(store.runs[0].level, 1);
    assert_eq!(
        store.get(&target).expect("read compacted newest"),
        Some(b"v3".to_vec())
    );
    assert_eq!(
        pinned.get(&target).expect("read pinned older"),
        Some(b"v2".to_vec())
    );

    // Drive L1 into L2: three more L0 cycles, then one L1 merge.
    store.append(&[put(key(3), b"b")]).expect("append frame 3");
    store.append(&[put(key(4), b"c")]).expect("append frame 4");
    store.append(&[put(target, b"v4")]).expect("append frame 5");
    store.append(&[put(key(5), b"d")]).expect("append frame 6");
    store.append(&[put(key(6), b"e")]).expect("append frame 7");
    store.append(&[put(key(7), b"f")]).expect("append frame 8");
    assert_eq!(store.runs.len(), 1);
    assert_eq!(store.runs[0].level, 2);
    assert_eq!(store.runs[0].min_epoch, 0);
    assert_eq!(store.runs[0].max_epoch, 8);
    assert_eq!(
        store.get(&target).expect("read L2 newest"),
        Some(b"v4".to_vec())
    );
    assert_eq!(
        pinned.get(&target).expect("pinned still older"),
        Some(b"v2".to_vec())
    );

    let stats = store.compaction_stats();
    assert_eq!(stats.cycles, 4);
    assert_eq!(stats.runs_merged, 12);
    assert_eq!(stats.runs_produced, 4);
    assert!(
        stats.output_records < stats.input_records,
        "dedup must drop superseded versions: {stats:?}"
    );
    drop(pinned);
    drop(store);

    let reopened =
        PackStore::open(root.path(), store_config(1024 * 1024)).expect("reopen compacted store");
    assert_eq!(
        reopened.get(&target).expect("read reopened L2"),
        Some(b"v4".to_vec())
    );
    assert_eq!(reopened.open_validation().runs, 1);
    assert_eq!(reopened.open_validation().frames, 9);
}

#[test]
fn metrics_cover_store_and_snapshot_reads_without_dynamic_labels() {
    let root = tempdir().expect("temporary append store");
    let mut store =
        PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
    let first = key(1);
    let second = key(2);
    let absent = key(3);
    store
        .append(&[put(first, b"one"), put(second, b"two")])
        .expect("append values");

    let snapshot = store.snapshot().expect("pin snapshot");
    assert_eq!(
        store.get(&first).expect("store point read"),
        Some(b"one".to_vec())
    );
    assert_eq!(store.get(&absent).expect("store miss"), None);
    assert_eq!(
        snapshot.get(&second).expect("snapshot point read"),
        Some(b"two".to_vec())
    );
    let keys = [first, second, absent];
    let values = snapshot
        .get_many_sorted(&keys)
        .expect("snapshot sorted read");
    assert_eq!(
        values,
        vec![Some(b"one".to_vec()), Some(b"two".to_vec()), None]
    );

    let metrics = store.metrics().expect("metrics snapshot");
    assert_eq!(metrics.append.frames, 1);
    assert_eq!(metrics.append.index_entries, 2);
    assert!(metrics.logical_payload_bytes > 0);
    assert_eq!(metrics.reads.point_reads, 3);
    assert_eq!(metrics.reads.point_hits, 2);
    assert_eq!(metrics.reads.point_misses, 1);
    assert_eq!(metrics.reads.sorted_batches, 1);
    assert_eq!(metrics.reads.sorted_keys, 3);
    assert_eq!(metrics.reads.sorted_hits, 2);
    assert_eq!(metrics.reads.sorted_value_bytes, 6);
    assert_eq!(metrics.live_runs, 1);
    assert!(!metrics.debt.backpressure_required);
    assert!(metrics.physical_layout_amplification_milli().is_some());
}

include!("compaction_recovery_tests.rs");
include!("crash_failpoint_tests.rs");
include!("evidence_tests.rs");
